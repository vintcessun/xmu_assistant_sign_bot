use crate::{
    abi::message::{MessageSend, MessageSendBuilder, file::FileUrl},
    api::network::{FutureFile, SessionClient, download_to_file_sync},
    config::get_self_qq,
};
use anyhow::Result;
use ego_tree::NodeRef;
use futures::future::join_all;
use scraper::{Html, Node};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::task::block_in_place;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlParseResult {
    pub message: MessageSendBuilder,
    pub markdown: String,
}

impl HtmlParseResult {
    pub fn new() -> Self {
        Self {
            message: MessageSend::new_message(),
            markdown: String::new(),
        }
    }

    pub fn text<S: Into<String>>(&mut self, text: S) {
        let text_string: String = text.into();
        self.markdown.push_str(&text_string);
        let builder = std::mem::take(&mut self.message);
        self.message = builder.text(text_string);
    }

    pub fn extend(&mut self, other: Self) {
        self.message.extend(other.message);
        self.markdown.push_str(&other.markdown);
    }

    /// 从另一个创建一个独立消息的内容
    pub fn node_message(&mut self, other: Self) {
        let message = other.message.build();
        let builder = std::mem::take(&mut self.message);
        self.message = builder.node_content(get_self_qq(), "查题助手", message);
        self.markdown.push_str(&other.markdown);
        self.markdown.push_str("\n\n");
    }
}

pub struct HtmlParseResultInner {
    pub message: MessageSendBuilder,
    pub markdown: String,
    pub download_tasks: Vec<FutureFile>,
}

impl HtmlParseResultInner {
    pub fn new() -> Self {
        Self {
            message: MessageSend::new_message(),
            markdown: String::new(),
            download_tasks: Vec::new(),
        }
    }

    pub fn text<S: Into<String>>(&mut self, text: S) {
        let text_string: String = text.into();
        self.markdown.push_str(&text_string);
        let builder = std::mem::take(&mut self.message);
        self.message = builder.text(text_string);
    }

    pub fn image_file(&mut self, file: FutureFile) -> Result<()> {
        self.image(FileUrl::from_path(&file.path)?);
        self.download_tasks.push(file);
        Ok(())
    }

    pub fn image(&mut self, url: FileUrl) {
        self.markdown
            .push_str(&format!("![image]({})", url.get_url()));
        let builder = std::mem::take(&mut self.message);
        self.message = builder.image(url);
    }

    pub fn extend(&mut self, other: Self) {
        self.message.extend(other.message);
        self.markdown.push_str(&other.markdown);
        self.download_tasks.extend(other.download_tasks);
    }
}

fn get_image(url: &str, client: Arc<SessionClient>) -> FutureFile {
    let filename = format!("{}", uuid::Uuid::new_v4());
    download_to_file_sync(client, url, &filename)
}

fn map_chars(text: &str, map_type: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        let mapped = match map_type {
            "sup" => match c {
                '-' => "⁻",
                '0' => "⁰",
                '1' => "¹",
                '2' => "²",
                '3' => "³",
                '4' => "⁴",
                '5' => "⁵",
                '6' => "⁶",
                '7' => "⁷",
                '8' => "⁸",
                '9' => "⁹",
                _ => "",
            },
            "sub" => match c {
                '-' => "₋",
                '0' => "₀",
                '1' => "¹",
                '2' => "₂",
                '3' => "₃",
                '4' => "₄",
                '5' => "₅",
                '6' => "₆",
                '7' => "₇",
                '8' => "₈",
                '9' => "₉",
                _ => "",
            },
            _ => "",
        };
        if !mapped.is_empty() {
            result.push_str(mapped);
        } else {
            result.push(c);
        }
    }
    result
}

#[derive(Clone)]
struct HtmlParser {
    client: Arc<SessionClient>,
}

impl HtmlParser {
    fn new(client: Arc<SessionClient>) -> Self {
        Self { client }
    }

    // 主入口
    pub async fn parse(&self, html_content: &str) -> Result<HtmlParseResult> {
        let html_content = html_content.to_string();

        let task = block_in_place(|| self.parse_inner(&html_content))?;

        let files = task.download_tasks;
        let futures = files.into_iter().map(|f| f.future);
        join_all(futures).await;

        let ret = HtmlParseResult {
            message: task.message,
            markdown: task.markdown,
        };

        Ok(ret)
    }

    pub fn parse_inner(&self, html_content: &str) -> Result<HtmlParseResultInner> {
        let document = Html::parse_fragment(html_content);
        let mut ret = HtmlParseResultInner::new();

        // 遍历根节点
        for node in document.root_element().children() {
            // 跳过根文档节点，只处理子节点
            if let Node::Document = node.value() {
                for child in node.children() {
                    let sub_ele = self.process_node(child)?;
                    ret.extend(sub_ele);
                }
            } else {
                // 如果是片段，可能直接就是节点
                let sub_ele = self.process_node(node)?;
                ret.extend(sub_ele);
            }
        }

        Ok(ret)
    }

    // 递归处理节点
    // 返回 (Vec<SegmentSend>, String) -> (消息段列表, Markdown文本)
    fn process_node(&self, node: NodeRef<'_, Node>) -> Result<HtmlParseResultInner> {
        let mut ret = HtmlParseResultInner::new();

        match node.value() {
            // 1. 纯文本
            Node::Text(text) => {
                let t = text.trim(); // 注意：根据需求决定是否 trim
                if !t.is_empty() {
                    ret.text(t);
                } else if text.contains('\n') {
                    ret.text('\n');
                }
            }

            // 2. 元素节点
            Node::Element(elem) => {
                match elem.name() {
                    // --- 图片 ---
                    "img" => {
                        if let Some(src) = elem.attr("src") {
                            let local_file = get_image(src, self.client.clone());
                            ret.image_file(local_file)?;
                        }
                    }

                    // --- 换行 ---
                    "br" => {
                        ret.text('\n');
                    }

                    // --- 代码块 ---
                    "pre" => {
                        let mut code_text = String::new();
                        // 尝试寻找内部 code 标签
                        let mut found = false;
                        for child in node.children() {
                            if let Node::Element(child_elem) = child.value() {
                                //处理代码块
                                if child_elem.name() == "code" {
                                    //如果有文本
                                    if let Some(text) = child.value().as_text() {
                                        code_text = text.to_string();
                                        found = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !found {
                            // 没找到 code 标签，直接取 pre 内文本
                            if let Some(text) = node.value().as_text() {
                                code_text = text.to_string();
                            }
                        }

                        let fmt_code = format!("```\n{}\n```", code_text);
                        ret.text(fmt_code);
                    }

                    // --- 上标/下标 ---
                    tag @ ("sup" | "sub") => {
                        if let Some(raw_text) = node.value().as_text() {
                            let mapped = map_chars(raw_text, tag);
                            ret.text(mapped);
                        }
                    }

                    // --- 列表 (ul/ol) ---
                    tag @ ("ul" | "ol") => {
                        let is_ordered = tag == "ol";
                        let mut index = 1;

                        for child in node.children() {
                            if let Node::Element(child_elem) = child.value() {
                                // 递归处理 li 内部内容
                                if child_elem.name() == "li" {
                                    let sub_li = self.process_node(child)?;

                                    // 前缀
                                    let prefix = if is_ordered {
                                        format!("{}. ", index)
                                    } else {
                                        "- ".to_string()
                                    };

                                    ret.text(prefix);
                                    ret.extend(sub_li);

                                    if is_ordered {
                                        index += 1;
                                    }
                                }
                            }
                        }
                    }

                    // --- 链接 ---
                    "a" => {
                        let href = elem.attr("href").unwrap_or("");
                        if let Some(text) = node.value().as_text() {
                            let text_content = text.trim();
                            let fmt = format!("{}({})", text_content, href);
                            ret.text(fmt);
                        }
                    }

                    // --- 填空题空白 ---
                    "span" if elem.attr("class").is_some_and(|c| c.contains("__blank__")) => {
                        // 寻找内部 class="circle-number"
                        let mut num = "".to_string();
                        for child in node.children() {
                            if let Node::Element(c_elem) = child.value() {
                                //内部查找
                                if c_elem.name() == "span"
                                    && c_elem
                                        .attr("class")
                                        .is_some_and(|c| c.contains("circle-number"))
                                {
                                    //获取数字文本
                                    if let Some(text) = child.value().as_text() {
                                        num = text.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                        let text = format!("__({})__", num);
                        ret.text(text);
                    }

                    // --- 默认递归 (div, p, span 等) ---
                    _ => {
                        for child in node.children() {
                            let sub_ret = self.process_node(child)?;
                            ret.extend(sub_ret);
                        }
                    }
                }
            }

            _ => {} // 注释等其他节点忽略
        }

        Ok(ret)
    }
}

// 对外暴露的便捷函数
pub async fn html_to_message_and_markdown(
    html: &str,
    client: Arc<SessionClient>,
) -> Result<HtmlParseResult> {
    let parser = HtmlParser::new(client);
    parser.parse(html).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::xmu_service::lnt::distribute::DistributeResponse;

    const SRC_JSON: &str = r#"{"exam_paper_instance_id":1381311,"subjects":[{"answer_number":0,"data":{},"description":"\u003Cp\u003E（  ）给中国送来了马克思列宁主义，给苦苦探寻救亡图存出路的中国人民指明了前进方向、提供了全新选择。\u003C/p\u003E","difficulty_level":"easy","id":1130976,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E鸦片战争\u003C/p\u003E","id":3836421,"sort":0,"type":"text"},{"content":"\u003Cp\u003E新文化运动\u003C/p\u003E","id":3836424,"sort":1,"type":"text"},{"content":"\u003Cp\u003E五四运动\u003C/p\u003E","id":3836427,"sort":2,"type":"text"},{"content":"\u003Cp\u003E十月革命\u003C/p\u003E","id":3836430,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"case_sensitive":true,"has_audio":false,"option_type":"text","options_layout":"vertical","play_limit":true,"play_limit_times":1,"required":false,"unordered":false,"uploads":[]},"sort":0,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E1938年，（  ）在党的六届六中全会上作了《论新阶段》的报告，强调：“没有抽象的马克思主义，只有具体的马克思主义……”\u003C/p\u003E","difficulty_level":"easy","id":1130979,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东\u003C/p\u003E","id":3836433,"sort":0,"type":"text"},{"content":"\u003Cp\u003E任弼时\u003C/p\u003E","id":3836436,"sort":1,"type":"text"},{"content":"\u003Cp\u003E刘少奇\u003C/p\u003E","id":3836439,"sort":2,"type":"text"},{"content":"\u003Cp\u003E周恩来\u003C/p\u003E","id":3836442,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":1,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E党的十八大以来，以习近平同志为核心的党中央明确提出要不断推进（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1130982,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E社会主义现代化\u003C/p\u003E","id":3836445,"sort":0,"type":"text"},{"content":"\u003Cp\u003E马克思主义中国化时代化\u003C/p\u003E","id":3836448,"sort":1,"type":"text"},{"content":"\u003Cp\u003E“两个结合”\u003C/p\u003E","id":3836451,"sort":2,"type":"text"},{"content":"\u003Cp\u003E社会主义现代化强国\u003C/p\u003E","id":3836454,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":2,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E（   ）明确把“不断谱写马克思主义中国化时代化新篇章”作为当代中国共产党人的庄严历史责任，并提出了继续推进马克思主义中国化时代化的新要求。\u003C/p\u003E","difficulty_level":"easy","id":1130985,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E党的十七大\u003C/p\u003E","id":3836457,"sort":0,"type":"text"},{"content":"\u003Cp\u003E党的十八大\u003C/p\u003E","id":3836460,"sort":1,"type":"text"},{"content":"\u003Cp\u003E党的十九大\u003C/p\u003E","id":3836463,"sort":2,"type":"text"},{"content":"\u003Cp\u003E党的二十大\u003C/p\u003E","id":3836466,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":3,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E（  ）是马克思主义中国化时代化的第一次历史性飞跃。\u003C/p\u003E","difficulty_level":"easy","id":1130988,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836469,"sort":0,"type":"text"},{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836472,"sort":1,"type":"text"},{"content":"\u003Cp\u003E“三个代表”重要思想\u003C/p\u003E","id":3836475,"sort":2,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836478,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":4,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的理论成果是一脉相承又（    ）的关系。\u003C/p\u003E","difficulty_level":"easy","id":1130991,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E实事求是\u003C/p\u003E","id":3836481,"sort":0,"type":"text"},{"content":"\u003Cp\u003E与时俱进\u003C/p\u003E","id":3836484,"sort":1,"type":"text"},{"content":"\u003Cp\u003E独立自主\u003C/p\u003E","id":3836487,"sort":2,"type":"text"},{"content":"\u003Cp\u003E精益求精\u003C/p\u003E","id":3836490,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":5,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的最新理论成果是（    ）。\u003C/p\u003E","difficulty_level":"easy","id":1130994,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836493,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836496,"sort":1,"type":"text"},{"content":"\u003Cp\u003E习近平新时代中国特色社会主义思想\u003C/p\u003E","id":3836499,"sort":2,"type":"text"},{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836502,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":6,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E（    ）的召开，实现了新中国成立以来党的历史上具有深远意义的伟大转折，开启了改革开放和社会主义现代化建设历史新时期。\u003C/p\u003E","difficulty_level":"easy","id":1130997,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E中共十一届三中全会 \u003C/p\u003E","id":3836505,"sort":0,"type":"text"},{"content":"\u003Cp\u003E中共十二大\u003C/p\u003E","id":3836508,"sort":1,"type":"text"},{"content":"\u003Cp\u003E中共十三大\u003C/p\u003E","id":3836511,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中共十一届六中全会\u003C/p\u003E","id":3836514,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":7,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E中国特色社会主义理论体系不包括（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131000,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836517,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836520,"sort":1,"type":"text"},{"content":"\u003Cp\u003E习近平新时代中国特色社会主义思想\u003C/p\u003E","id":3836523,"sort":2,"type":"text"},{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836526,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":8,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E中国共产党一经诞生，就把为中国人民谋幸福、为中华民族谋复兴确立为自己的初心使命。一百年来，中国共产党团结带领中国人民进行的一切奋斗、一切牺牲、一切创造，归结起来就是一个主题（ ）。\u003C/p\u003E","difficulty_level":"easy","id":1131003,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E实现共同富裕\u003C/p\u003E","id":3836529,"sort":0,"type":"text"},{"content":"\u003Cp\u003E实现全面建成小康社会\u003C/p\u003E","id":3836532,"sort":1,"type":"text"},{"content":"\u003Cp\u003E实现社会主义现代化\u003C/p\u003E","id":3836535,"sort":2,"type":"text"},{"content":"\u003Cp\u003E实现中华民族伟大复兴\u003C/p\u003E","id":3836538,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":9,"sub_subjects":[],"type":"single_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E准确把握马克思主义中国化时代化的科学内涵，要做到坚持（  ）与（  ）相统一。\u003C/p\u003E","difficulty_level":"easy","id":1131006,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义\u003C/p\u003E","id":3836541,"sort":0,"type":"text"},{"content":"\u003Cp\u003E发展马克思主义 \u003C/p\u003E","id":3836544,"sort":1,"type":"text"},{"content":"\u003Cp\u003E社会主义\u003C/p\u003E","id":3836547,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中国特色社会主义\u003C/p\u003E","id":3836550,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":10,"sub_subjects":[],"type":"multiple_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E推进马克思主义中国化时代化，是（   ）。\u003C/p\u003E","difficulty_level":"easy","id":1131009,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义唯物史观的要求\u003C/p\u003E","id":3836553,"sort":0,"type":"text"},{"content":"\u003Cp\u003E马克思主义理论本身发展的内在要求\u003C/p\u003E","id":3836556,"sort":1,"type":"text"},{"content":"\u003Cp\u003E解决中国实际问题的客观需要\u003C/p\u003E","id":3836559,"sort":2,"type":"text"},{"content":"\u003Cp\u003E社会主义经济社会发展的需要\u003C/p\u003E","id":3836562,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":11,"sub_subjects":[],"type":"multiple_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E坚持和发展马克思主义，必须（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131012,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E同中国具体实际相结合\u003C/p\u003E","id":3836565,"sort":0,"type":"text"},{"content":"\u003Cp\u003E同中华优秀传统文化相结合\u003C/p\u003E","id":3836568,"sort":1,"type":"text"},{"content":"\u003Cp\u003E同社会主义现代化发展相结合\u003C/p\u003E","id":3836571,"sort":2,"type":"text"},{"content":"\u003Cp\u003E同中华民族伟大复兴相结合\u003C/p\u003E","id":3836574,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":12,"sub_subjects":[],"type":"multiple_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E要坚持解放思想、实事求是、与时俱进、求真务实，一切从实际出发，着眼解决革命、建设、改革中的实际问题，不断回答（  ），作出符合中国实际和时代要求的正确回答，得出符合客观规律的科学认识，形成与时俱进的理论成果，更好指导中国实践。\u003C/p\u003E","difficulty_level":"easy","id":1131015,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E中国之问\u003C/p\u003E","id":3836577,"sort":0,"type":"text"},{"content":"\u003Cp\u003E世界之问\u003C/p\u003E","id":3836580,"sort":1,"type":"text"},{"content":"\u003Cp\u003E人民之问\u003C/p\u003E","id":3836583,"sort":2,"type":"text"},{"content":"\u003Cp\u003E时代之问\u003C/p\u003E","id":3836586,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":13,"sub_subjects":[],"type":"multiple_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E实践证明，中国共产党为什么能，中国特色社会主义为什么好，归根到底是（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131018,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义经典作家行\u003C/p\u003E","id":3836589,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学社会主义行\u003C/p\u003E","id":3836592,"sort":1,"type":"text"},{"content":"\u003Cp\u003E马克思主义行\u003C/p\u003E","id":3836595,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中国化时代化的马克思主义行\u003C/p\u003E","id":3836598,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":14,"sub_subjects":[],"type":"multiple_selection"},{"answer_number":0,"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的内涵是（）\u003C/p\u003E","difficulty_level":"easy","id":1131021,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E就是立足中国国情和时代特点\u003C/p\u003E","id":3836601,"sort":0,"type":"text"},{"content":"\u003Cp\u003E坚持把马克思主义基本原理同中国具体实际相结合、同中华优秀传统文化相结合，\u003C/p\u003E","id":3836604,"sort":1,"type":"text"},{"content":"\u003Cp\u003E深入研究和解决中国革命、建设、改革不同历史时期的实际问题\u003C/p\u003E","id":3836607,"sort":2,"type":"text"},{"content":"\u003Cp\u003E真正搞懂面临的时代课题，不断吸收新的时代内容，科学回答时代提出的重大理论和实践课题，创造新的理论成果。\u003C/p\u003E","id":3836610,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":15,"sub_subjects":[],"type":"multiple_selection"}]}"#;
    #[tokio::test]
    async fn test() -> Result<()> {
        let res = serde_json::from_str::<DistributeResponse>(SRC_JSON)?;
        println!("{:?}", res);
        let client = Arc::new(SessionClient::new());
        let msg = res.parse(client).await?;
        println!("{:?}", msg);
        Ok(())
    }

    #[tokio::test]
    async fn test_description() -> Result<()> {
        let html = "<p>（  ）给中国送来了马克思列宁主义，给苦苦探寻救亡图存出路的中国人民指明了前进方向、提供了全新选择。</p>";

        let document = Html::parse_fragment(html);

        for node in document.root_element().children() {
            // 跳过根文档节点，只处理子节点
            if let Node::Document = node.value() {
                for child in node.children() {
                    println!("Child: {:?}", child.value());
                }
            } else {
                // 如果是片段，可能直接就是节点
                println!("Node: {:?}", node.value());
            }
        }

        println!("{:?}", document.tree.nodes());
        Ok(())
    }
}
