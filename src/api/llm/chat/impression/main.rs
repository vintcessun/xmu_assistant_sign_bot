use crate::api::llm::tool::LlmPrompt;
use crate::api::llm::tool::{LlmHashMap, LlmVec, ask_as};
use crate::api::storage::ColdTable;
use anyhow::Result;
use dashmap::DashMap;
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};

static IMPRESSION_DB: LazyLock<ColdTable<i32, Impression>> =
    LazyLock::new(|| ColdTable::new("llm_impression_storage"));

// 印象结构体定义
#[derive(Debug, Serialize, Deserialize, Clone, LlmPrompt)]
pub struct Impression {
    // 1. 核心定性字段 (LLM 最擅长的描述)
    #[prompt("一句简短精炼的话总结对该用户的整体感觉和关键特征。")]
    pub summary: String,
    #[prompt("提取描述用户个性的关键词标签，例如 幽默, 有攻击性, 博学。请提供 3 到 5 个。")]
    pub personality_tags: LlmVec<String>,

    // 2. 关系定位
    #[prompt("与 Bot 之间的关系阶段，如: 陌生, 熟络, 产生信任, 冲突中。")]
    pub relationship_stage: String,
    #[prompt("观察到该用户偏好的聊天语气风格，如: 直白, 委婉, 二次元, 专业。")]
    pub tone_preference: String,

    // 3. 核心心理维度 (让 LLM 填 "极高/中等/偏低" 或详细描述)
    #[prompt("对用户的亲和度描述，请填写 极高, 中等, 偏低 或进行详细描述。")]
    pub warmth_level: String,
    #[prompt("对用户认知能力/专业度/逻辑思维能力的评价，请填写 极高, 中等, 偏低 或进行详细描述。")]
    pub competence_level: String,

    // 4. 动态记忆锚点
    #[prompt("导致当前印象发生质变（正向或负向）的关键对话点或转折事件的简短描述。")]
    pub key_interaction_moment: String,
    #[prompt("推测用户聊天动机（想寻求帮助？想寻找认同？想表达观点？想挑衅？）。")]
    pub user_motivation: String,

    // 5. 行为倾向预判 (由 LLM 预测 Bot 下一步该怎么应对)
    #[prompt(
        "建议 Bot 针对此用户采取的对话策略和姿态，如: 保持距离, 主动示好, 严肃讨论, 积极引导。"
    )]
    pub strategy_suggestion: String,

    // 6. 扩展槽位 (应对 LLM 偶尔生成的额外信息)
    #[prompt(
        "LLM 可能会生成的额外、难以分类的用户特征信息，以键值对形式存储，例如 {\"兴趣爱好\": \"旅行\"}。"
    )]
    pub extended_traits: LlmHashMap<String, String>,
}

// ------------------------------------
// 印象管理状态
// ------------------------------------

// 存储每个用户的消息计数和下次更新的阈值
// Key: User ID (i32)
#[derive(Debug, Clone)]
pub struct UserImpressionState {
    pub message_count: usize,
    pub update_threshold: usize,           // 随机数 50-100
    pub message_history: Vec<ChatMessage>, // 临时存储消息历史，用于生成印象
}

static IMPRESSION_STATE: LazyLock<DashMap<i32, UserImpressionState>> = LazyLock::new(DashMap::new);

fn generate_threshold() -> usize {
    rand::rng().random_range(50..=100)
}

/// 每次收到 router 消息时调用，用于记录消息并检查是否触发印象更新
/// message 假设是用户发送的消息文本
pub async fn push_message(user_id: i32, message: ChatMessage) -> Result<()> {
    let state_map = &*IMPRESSION_STATE;

    let mut state = state_map
        .entry(user_id)
        .or_insert_with(|| UserImpressionState {
            message_count: 0,
            update_threshold: generate_threshold(),
            message_history: Vec::new(),
        });

    state.message_count += 1;
    state.message_history.push(message);

    if state.message_count >= state.update_threshold {
        info!(
            user_id = ?user_id,
            count = ?state.message_count,
            threshold = ?state.update_threshold,
            "用户印象更新已触发，历史消息数达到阈值"
        );

        // 提取历史记录并重置状态
        let history = state.message_history.clone();
        state.message_history.clear(); // 清空历史
        state.message_count = 0;
        state.update_threshold = generate_threshold();

        drop(state);

        if let Err(e) = update_impression(user_id, history).await {
            error!(user_id = ?user_id, error = ?e, "更新用户印象失败");
        } else {
            info!(user_id = ?user_id, "用户印象更新成功");
        }
    }

    Ok(())
}

/// 实际执行印象生成或更新的逻辑
async fn update_impression(user_id: i32, history: Vec<ChatMessage>) -> Result<()> {
    let old_impression: Option<Impression> =
        IMPRESSION_DB.get_async(user_id).await.map_err(|e| {
            error!(user_id = ?user_id, error = ?e, "获取旧的用户印象失败");
            e
        })?;

    if old_impression.is_none() {
        debug!(user_id = ?user_id, "用户是新用户，开始生成新印象");
    } else {
        debug!(user_id = ?user_id, "用户存在旧印象，开始更新印象");
    }

    let prompt_content = if let Some(old) = &old_impression {
        [vec![ChatMessage::system(format!(
                "你是一个高度智能的AI助教，负责跟踪用户的交互历史并构建其心理档案，用于优化后续的对话策略。这是用户最近的{}条对话记录，你需要根据这些新的互动来更新现有的印象：",
                history.len()
            )),
            ChatMessage::system("近互动记录:")],
            history,
            vec![ChatMessage::system(format!(
                "这是该用户当前的印象（请仔细整合、更新和覆盖）：{:?}",
                old
            )),
            ChatMessage::system("请根据对话历史，结合旧印象，生成一份完整且更新后的用户印象。")]].concat()
    } else {
        [vec![
            ChatMessage::system(
                "你是一个高度智能的AI助教，负责跟踪用户的交互历史并构建其心理档案。请根据以下用户对话记录，生成一个全新的用户印象：\n对话记录:",
            )],
            history,
            vec![ChatMessage::system("请基于此生成一份完整的用户印象。请勿包含任何解释性文字")]].concat()
    };

    let new_impression = ask_as::<Impression>(prompt_content).await.map_err(|e| {
        error!(user_id = ?user_id, error = ?e, "LLM 生成新印象失败");
        e
    })?;

    IMPRESSION_DB
        .insert(user_id, new_impression)
        .await
        .map_err(|e| {
            error!(user_id = ?user_id, error = ?e, "保存新印象到数据库失败");
            e
        })?;

    debug!(user_id = ?user_id, "成功更新用户印象并保存");

    Ok(())
}

pub async fn get_impression(user_id: i32) -> Option<Impression> {
    debug!(user_id = ?user_id, "尝试获取用户印象");
    IMPRESSION_DB
        .get_async(user_id)
        .await
        .map_err(|e| {
            warn!(user_id = ?user_id, error = ?e, "获取用户印象失败");
            e
        })
        .ok()
        .flatten()
}
