//! 定时签到：按「同课分组蹲守」跑，而不是每个上课中的用户各自轮询。
//!
//! 一轮的流程：
//! 1. 找出处于「课程时间段前后十分钟」的活跃用户（判定口径与改动前完全一致）；
//! 2. **时段开始那一轮**给每个人做一次会话预热（探测 + 必要时重新登录），
//!    失效的照旧提醒一次——这一步的时机与改动前一致，省的是**课程进行中**的重复探测；
//! 3. 用选课索引把他们按课分组，[`plan_tick`] 按「最久没当过班」轮值挑出哨兵；
//! 4. 只有哨兵去打 `radar/rollcalls`（用预热好的缓存会话，不再重复验活），
//!    各自带随机抖动错开；
//! 5. 哨兵发现签到后，这门课的签到进度**整轮只查一次**（带 TTL 缓存），
//!    达到阈值才把同课的活跃用户派发去签；
//! 6. 派发时先让一个人跑完整流程，他会把签到码 / 雷达位置写进共享缓存，
//!    后面的人直接走缓存法，省掉逐点探测的一大串请求；
//! 7. 某人某场签到一旦确认落地就记账，后续轮次不再为他发任何请求。

use crate::abi::client::get_client;
use crate::abi::echo::Echo;
use crate::abi::message::MessageSend;
use crate::abi::message::api::SendGroupMessageParams;
use crate::abi::network::BotClient;
use crate::api::network::SessionClient;
use crate::api::xmu_service::lnt::rollcalls::{Rollcall, RollcallStatus};
use crate::api::xmu_service::lnt::{ProfileWithoutCache, Rollcalls};
use crate::logic::helper::{get_client_from_cache, get_client_or_err_for_id};
use crate::logic::rollcall::auto_sign_data::AutoSignResponse;
use crate::logic::rollcall::auto_sign_data::auto_sign_response::{NumberSign, QRSign, RadarSign};
use crate::logic::rollcall::course_index::{current_index, spawn_upsert};
use crate::logic::rollcall::data::LOGIN_DATA;
use crate::logic::rollcall::data::TIMETABLE_GROUP;
use crate::logic::rollcall::sign_data::get_on_call_total_num;
use crate::logic::rollcall::spec_sign::spec_sign_request_inner;
use crate::logic::rollcall::watch::{TickPlan, plan_tick};
use crate::{
    api::{
        scheduler::{TaskRunner, TimeTask},
        xmu_service::jw::ClockTime,
    },
    logic::rollcall::{
        time::{TIME_SIGN_TASK, class_codes_in_session_now},
        timetable::all_timetable_users,
        utils::uniform,
    },
};
use anyhow::Result;
use async_trait::async_trait;
use dashmap::{DashMap, DashSet};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use std::{sync::LazyLock, time::Duration};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

/// 班上签到人数达到这个百分比之后才动手，沿用改动前的阈值。
const SIGN_THRESHOLD_PERCENT: usize = 15;
/// 哨兵轮询的随机抖动窗口（秒），把每轮的请求在时间上散开。
const SCOUT_SPREAD_SECS: u64 = 8;
/// 派发签到时非领签者的随机抖动窗口（毫秒）。
const SIGN_SPREAD_MS: u64 = 2500;
/// 等领签者的上限；超时就不再等（任务继续在后台跑完，本轮末尾再收结果）。
const LEADER_TIMEOUT: Duration = Duration::from_secs(20);
/// 签到进度缓存的有效期，略短于一轮，保证每轮都是新数据但一轮内只查一次。
const PROGRESS_TTL: Duration = Duration::from_secs(15);
/// 「这场签到这个人已经搞定」的记账保留时长，过期清理避免无限增长。
const SIGNED_MARK_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// 发现索引落后后重新拉一次选课的冷却时间，避免每轮都为同一个人重复拉。
const HEAL_COOLDOWN: Duration = Duration::from_secs(30 * 60);
/// 与上一次活跃隔了这么久，就认为是一个新的上课时段开始，要重新做一次会话预热。
/// 取值要大于一轮的最长耗时（间隔 40s + 抖动 + 领签等待），否则同一时段里会被误判成新时段。
const SESSION_GAP: Duration = Duration::from_secs(3 * 60);
/// 预热失败后的重试间隔：不是整个时段放弃，中途重新登录的人还能被捞回来。
const WARM_RETRY: Duration = Duration::from_secs(5 * 60);
/// 预热结论的最长复用时长。连堂课会把活跃窗口连成一大片，这时也别一直吃老结论。
const WARM_MAX_AGE: Duration = Duration::from_secs(30 * 60);
/// 会话预热的随机抖动窗口（秒）。时段开始时所有人都要预热，更要摊开。
const WARM_SPREAD_SECS: u64 = 10;

/// 记录已因登录失效而提醒过的用户，避免定时任务每轮重复提醒。
/// 登录恢复正常后会移除对应用户，下次再失效时可重新提醒一次。
/// 仅存于内存：进程重启后清空，重启后首次失效会再提醒一次，符合“提醒一次”的预期。
static NOTLOGIN_REMINDED: LazyLock<DashSet<i64>> = LazyLock::new(DashSet::new);

/// `(qq, rollcall_id) -> 记账时刻`。确认签到落地后写入，之后不再为这个人发任何请求。
static SIGNED_MARK: LazyLock<DashMap<(i64, i64), Instant>> = LazyLock::new(DashMap::new);

/// `rollcall_id -> (查询时刻, 已签人数, 总人数)`。同一场签到一轮内只查一次。
static PROGRESS_CACHE: LazyLock<DashMap<i64, (Instant, usize, usize)>> =
    LazyLock::new(DashMap::new);

/// `qq -> 上次因索引落后而补拉选课的时刻`。
static HEAL_TRIED: LazyLock<DashMap<i64, Instant>> = LazyLock::new(DashMap::new);

/// 上一次播报过的 v3 存量人数，只在变化时才打日志，不刷屏。
static LAST_LEGACY_COUNT: AtomicU64 = AtomicU64::new(u64::MAX);

/// 播报还有多少人留在 v3 课表上。人数变化才打一条，归零时那条就是
/// 「可以删掉 legacy 模块和 v3 表了」的信号。
///
/// v3 空了之后这个函数连同 `legacy_v3_count` 一起删掉。
fn report_legacy_drain() {
    let count = super::timetable::legacy_v3_count() as u64;
    if LAST_LEGACY_COUNT.swap(count, Ordering::Relaxed) == count {
        return;
    }
    if count == 0 {
        info!(
            legacy_v3_users = 0,
            "v3 课表存量已清空，可以移除 legacy 兼容代码与 logic_command_sign_time_v3 表"
        );
    } else {
        info!(
            legacy_v3_users = count,
            "仍有用户停留在 v3 课表，等他们重新 /signtime"
        );
    }
}

/// 通用的按人限流：距上次超过 `cooldown` 才放行，并记下这一次。
fn log_cooldown_passed(table: &DashMap<i64, Instant>, qq: i64, cooldown: Duration) -> bool {
    if table
        .get(&qq)
        .is_some_and(|t| t.value().elapsed() < cooldown)
    {
        return false;
    }
    table.insert(qq, Instant::now());
    true
}

/// 轮次号，每跑一轮 +1，用来记录“谁上次是第几轮当的班”。
static TICK_SEQ: AtomicU64 = AtomicU64::new(0);

/// `qq -> 上次当哨兵的轮次号`。排班优先挑这个值最小（最久没当过）的人，
/// 让轮询请求在所有活跃用户之间轮着摊，而不是固定压在选课最多的那几个人头上。
static SCOUT_DUTY: LazyLock<DashMap<i64, u64>> = LazyLock::new(DashMap::new);

/// `qq -> 上次看到他活跃的时刻`，用来判定“新的上课时段开始了”。
static LAST_ACTIVE: LazyLock<DashMap<i64, Instant>> = LazyLock::new(DashMap::new);

/// `qq -> (上次预热时刻, 会话是否可用)`。时段内沿用这个结论，不再逐轮验活。
static WARM_STATE: LazyLock<DashMap<i64, (Instant, bool)>> = LazyLock::new(DashMap::new);

pub struct TimeSignTask;

#[async_trait]
impl TimeTask for TimeSignTask {
    type Output = ();

    fn interval(&self) -> Duration {
        Duration::from_secs(uniform(20..40))
    }

    fn name(&self) -> &'static str {
        "TimeSignTask"
    }

    async fn run(&self) -> Result<Self::Output> {
        time_sign_task().await?;
        Ok(())
    }
}

/// 哨兵一次轮询的结果。
enum ScoutOutcome {
    Ok {
        qq: i64,
        client: SessionClient,
        rollcalls: Vec<Rollcall>,
    },
    /// 会话恢复不了，等于这个人本轮什么都做不了。
    NotLogin { qq: i64 },
    /// 会话没问题但请求失败，可以让同课的其他人补位。
    Failed { qq: i64 },
}

/// 单个用户一次签到尝试的结果。
enum SignOutcome {
    Done {
        qq: i64,
        responses: Vec<AutoSignResponse>,
    },
    NotLogin {
        qq: i64,
    },
    Failed,
}

/// 哨兵发现的一场签到，附带发现者的会话（查进度时直接复用，不用再恢复一次登录）。
struct Found {
    course_id: i64,
    scout: i64,
    client: SessionClient,
}

/// 一轮里从哨兵那里汇总到的东西。
#[derive(Default)]
struct Gathered {
    /// `rollcall_id -> 发现详情`，多个哨兵看到同一场签到时只留一份。
    found: HashMap<i64, Found>,
    /// 本轮确实被人盯过的 course_id。
    covered: HashSet<i64>,
    /// 会话恢复失败、需要提醒重新登录的人。
    notlogin: HashSet<i64>,
    /// 本轮已经试过且失败的人，补位时跳过。
    failed: HashSet<i64>,
}

async fn time_sign_task() -> Result<()> {
    prune_caches();
    report_legacy_drain();

    let course_time = TIME_SIGN_TASK.get_latest().await?;

    // 活跃用户判定与改动前完全一致：课程时间段前后十分钟，且登记过播报群。
    let now = ClockTime::now();
    let mut active: Vec<i64> = Vec::new();
    let mut groups: HashMap<i64, i64> = HashMap::new();
    for qq in all_timetable_users() {
        let is_active = course_time
            .get(&qq)
            .map(|e| e.value().is_active(now))
            .unwrap_or(false);
        if !is_active {
            continue;
        }
        let Some(group_id) = TIMETABLE_GROUP.get(&qq) else {
            continue;
        };
        active.push(qq);
        groups.insert(qq, *group_id);
    }

    if active.is_empty() {
        return Ok(());
    }

    // 时段开始那一轮给每个人做一次会话预热（时机与改动前一致：每个时段之前必试一次），
    // 时段进行中沿用这次的结论，不再逐人逐轮验活——那才是要砍掉的重复请求。
    let (active, mut notlogin) = warm_up(&active).await;
    if active.is_empty() {
        send_reports(HashMap::new(), notlogin, &groups).await;
        return Ok(());
    }

    let index = current_index().await;

    // 要盯的课＝**此刻正处在自己蹲守窗口内的那几门**（每门课各自「上课时间前后十分钟」），
    // 靠教务的班级代码精确落到 lnt 的教学班上。
    //
    // 不能拿这个人 lnt 上的全部课程来分组：`my-courses` 返回的是历年所有课
    // （线上实测 79 人摊出 2043 门），往年的课基本没有第二个人选，混进来就会把
    // 要覆盖的集合撑爆，逼得每个人都自己当哨兵，同课分组等于白做。
    //
    // 班级代码为空（还没重新跑过 `/signtime` 的 v3 存量数据）或课不在索引里的，
    // 一律不进 courses_of —— 排班会让他自己当哨兵，他那条 rollcalls 照样能看到
    // 自己全部的签到，不会漏签，只是省不掉这条请求。
    let mut courses_of: HashMap<i64, Vec<i64>> = HashMap::new();
    // 另存一份该用户在 lnt 上的**全部**当前学期课程，供“索引落后”的自愈判断用：
    // 拿收窄后的集合去判断的话，任何一门不在上课的课都会被误当成索引落后。
    let mut all_courses_of: HashMap<i64, Vec<i64>> = HashMap::new();
    if let Some(idx) = &index {
        for &qq in &active {
            if let Some(all) = idx.courses_of_qq(qq) {
                all_courses_of.insert(qq, all);
            }
            let watched: Vec<i64> = class_codes_in_session_now(qq)
                .iter()
                .filter_map(|code| idx.id_of_class_code(code))
                .collect();
            if watched.is_empty() {
                continue;
            }
            courses_of.insert(qq, watched);
        }
    }
    let last_duty: HashMap<i64, u64> = active
        .iter()
        .filter_map(|&qq| SCOUT_DUTY.get(&qq).map(|v| (qq, *v.value())))
        .collect();
    let plan = plan_tick(&active, &courses_of, &last_duty);
    let tick = TICK_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    for &qq in &plan.scouts {
        SCOUT_DUTY.insert(qq, tick);
    }
    debug!(
        tick,
        active = active.len(),
        scouts = plan.scouts.len(),
        watched_courses = plan.members.len(),
        "定时签到蹲守排班"
    );

    let mut gathered = Gathered::default();
    let outcomes = run_scouts(&plan.scouts).await;
    gathered.absorb(outcomes, &courses_of, &all_courses_of);

    // 哨兵掉线意味着它盯的课本轮没人看，立刻从同课的其他活跃用户里补位一次，别整轮漏掉。
    let backups = pick_backups(&plan, &courses_of, &gathered);
    if !backups.is_empty() {
        debug!(backups = backups.len(), "哨兵失败，补位重试");
        for &qq in &backups {
            SCOUT_DUTY.insert(qq, tick);
        }
        let outcomes = run_scouts(&backups).await;
        gathered.absorb(outcomes, &courses_of, &all_courses_of);
    }

    let active_set: HashSet<i64> = active.iter().copied().collect();
    let mut reports: HashMap<i64, Vec<AutoSignResponse>> = HashMap::new();
    notlogin.extend(gathered.notlogin.iter().copied());
    let mut late: Vec<(i64, JoinHandle<SignOutcome>)> = Vec::new();

    for (rollcall_id, found) in gathered.found {
        // 先看还有谁需要签：都记过账就整场跳过，连进度都不用查。
        //
        // 派发名单是**广**口径：直接问索引“谁选了这门课”，而不是用收窄后的
        // plan.members。收窄只决定“本轮要派几个人去盯”，绝不能顺带缩小“发现签到后
        // 该替谁签”——否则课表名字没对上的那些课，同班同学就被漏掉了。
        let mut targets: Vec<i64> = index
            .as_ref()
            .and_then(|idx| idx.qq_of_course(found.course_id))
            .unwrap_or_default();
        if !targets.contains(&found.scout) {
            targets.push(found.scout);
        }
        targets.retain(|qq| active_set.contains(qq) && !is_signed(*qq, rollcall_id));
        if targets.is_empty() {
            continue;
        }

        // 进度查询整轮只发一条，而不是每个同课的人各发一条。
        let Some((sign_num, student_num)) = progress(&found.client, rollcall_id).await else {
            continue;
        };
        if sign_num < student_num * SIGN_THRESHOLD_PERCENT / 100 {
            trace!(
                rollcall_id,
                sign_num, student_num, "签到人数还不够，本轮不动手"
            );
            continue;
        }
        trace!(rollcall_id, targets = targets.len(), "准备派发定时签到");

        // 领签：先让一个人跑完整流程，把签到码 / 雷达位置写进共享缓存。
        // 优先让哨兵打头阵——他的会话刚用过，最不容易卡在恢复登录上。
        let leader_pos = targets
            .iter()
            .position(|&x| x == found.scout)
            .unwrap_or_default();
        let leader = targets.swap_remove(leader_pos);
        let mut handle = tokio::spawn(sign_one(leader, rollcall_id));
        match tokio::time::timeout(LEADER_TIMEOUT, &mut handle).await {
            Ok(Ok(outcome)) => absorb_sign(outcome, rollcall_id, &mut reports, &mut notlogin),
            Ok(Err(e)) => error!(qq = leader, rollcall_id, error = ?e, "领签任务异常退出"),
            Err(_) => {
                warn!(
                    qq = leader,
                    rollcall_id, "领签较慢，不再等待，其余人各自尝试"
                );
                late.push((rollcall_id, handle));
            }
        }

        if !targets.is_empty() {
            let mut tasks = Vec::with_capacity(targets.len());
            for qq in targets {
                tasks.push(async move {
                    // 简单随机错开，别在领签结束的瞬间又齐射一波。
                    tokio::time::sleep(Duration::from_millis(uniform(0..SIGN_SPREAD_MS))).await;
                    sign_one(qq, rollcall_id).await
                });
            }
            for outcome in futures::future::join_all(tasks).await {
                absorb_sign(outcome, rollcall_id, &mut reports, &mut notlogin);
            }
        }
    }

    // 收尾等一下没来得及的领签，别把它的播报丢了。
    for (rollcall_id, handle) in late {
        match handle.await {
            Ok(outcome) => absorb_sign(outcome, rollcall_id, &mut reports, &mut notlogin),
            Err(e) => error!(rollcall_id, error = ?e, "领签任务异常退出"),
        }
    }

    send_reports(reports, notlogin, &groups).await;

    Ok(())
}

impl Gathered {
    fn absorb(
        &mut self,
        outcomes: Vec<ScoutOutcome>,
        courses_of: &HashMap<i64, Vec<i64>>,
        all_courses_of: &HashMap<i64, Vec<i64>>,
    ) {
        for outcome in outcomes {
            let (qq, client, rollcalls) = match outcome {
                ScoutOutcome::NotLogin { qq } => {
                    self.notlogin.insert(qq);
                    self.failed.insert(qq);
                    continue;
                }
                ScoutOutcome::Failed { qq } => {
                    self.failed.insert(qq);
                    continue;
                }
                ScoutOutcome::Ok {
                    qq,
                    client,
                    rollcalls,
                } => (qq, client, rollcalls),
            };

            NOTLOGIN_REMINDED.remove(&qq);
            if let Some(cs) = courses_of.get(&qq) {
                self.covered.extend(cs.iter().copied());
            }

            for rollcall in rollcalls {
                // 二维码签到没法自动完成（要现场的码），改动前这条路径也只会产出被过滤掉的
                // qr_pending。跳过它可以省掉一次进度查询和一整轮无效派发。
                if !rollcall.is_number && !rollcall.is_radar {
                    continue;
                }
                heal_index_if_stale(qq, rollcall.course_id, all_courses_of);
                self.found.entry(rollcall.rollcall_id).or_insert(Found {
                    course_id: rollcall.course_id,
                    scout: qq,
                    client: client.clone(),
                });
            }
        }
    }
}

/// 哨兵查到了一门索引里没记在他名下的课，说明索引落后了，顺手补拉一次。
/// 只有真的发现不一致才会发请求，并且按人做冷却，不会每轮重复拉。
fn heal_index_if_stale(qq: i64, course_id: i64, courses_of: &HashMap<i64, Vec<i64>>) {
    let Some(known) = courses_of.get(&qq) else {
        return;
    };
    if known.contains(&course_id) {
        return;
    }
    if !log_cooldown_passed(&HEAL_TRIED, qq, HEAL_COOLDOWN) {
        return;
    }
    if let Some(lnt) = LOGIN_DATA.get(&qq).map(|e| e.lnt.clone()) {
        debug!(qq, course_id, "选课索引落后于实际，触发补拉");
        spawn_upsert(qq, lnt);
    }
}

/// 为「哨兵失败导致没人盯」的课挑补位者：跳过本轮已经失败的人，
/// 也跳过已被其它补位者顺带盖住的课。
fn pick_backups(
    plan: &TickPlan,
    courses_of: &HashMap<i64, Vec<i64>>,
    gathered: &Gathered,
) -> Vec<i64> {
    let mut backups: Vec<i64> = Vec::new();
    for (course_id, members) in &plan.members {
        if gathered.covered.contains(course_id) {
            continue;
        }
        let already = backups
            .iter()
            .any(|b| courses_of.get(b).is_some_and(|cs| cs.contains(course_id)));
        if already {
            continue;
        }
        if let Some(&pick) = members.iter().find(|qq| !gathered.failed.contains(qq)) {
            backups.push(pick);
        }
    }
    backups
}

/// 会话预热：**每个上课时段开始之前**给每个人试一次登录恢复，失效的照旧提醒。
///
/// 时机与改动前一致（改动前是每轮都试），砍掉的只是同一个时段里的重复探测：
/// 一节课的窗口有几十轮，改动前每轮每人一次 `profile` 验活，现在整段只有一次。
///
/// 返回 `(会话可用的活跃用户, 需要提醒重新登录的人)`。
async fn warm_up(active: &[i64]) -> (Vec<i64>, HashSet<i64>) {
    let mut need_warm = Vec::new();
    let mut cached_ok = Vec::new();
    let mut cached_bad = HashSet::new();

    for &qq in active {
        match warm_decision(qq) {
            WarmDecision::Reuse => cached_ok.push(qq),
            WarmDecision::Skip => {
                cached_bad.insert(qq);
            }
            WarmDecision::Warm => need_warm.push(qq),
        }
        LAST_ACTIVE.insert(qq, Instant::now());
    }

    if need_warm.is_empty() {
        return (cached_ok, cached_bad);
    }

    let mut tasks = Vec::with_capacity(need_warm.len());
    for qq in need_warm {
        tasks.push(async move {
            // 时段开始时是全员一起进来的，更要摊开，别在同一秒打一排登录探测。
            tokio::time::sleep(Duration::from_secs(uniform(0..WARM_SPREAD_SECS))).await;
            match get_client_or_err_for_id(qq).await {
                Ok(_) => (qq, true),
                Err(e) => {
                    debug!(qq, error = ?e, "时段开始的会话预热失败");
                    (qq, false)
                }
            }
        });
    }

    let mut ok = cached_ok;
    let mut bad = cached_bad;
    for (qq, alive) in futures::future::join_all(tasks).await {
        WARM_STATE.insert(qq, (Instant::now(), alive));
        if alive {
            NOTLOGIN_REMINDED.remove(&qq);
            ok.push(qq);
        } else {
            bad.insert(qq);
        }
    }
    (ok, bad)
}

/// 本轮该拿这个人怎么办。
#[derive(Debug, PartialEq, Eq)]
enum WarmDecision {
    /// 同一时段里已经预热成功，直接用缓存会话，本轮不为他发验活请求。
    Reuse,
    /// 同一时段里预热失败过且还没到重试点，本轮完全跳过，不骚扰也不重复提醒。
    Skip,
    /// 新时段开始 / 从没见过 / 失败重试点到了：做一次完整的登录恢复。
    Warm,
}

/// 预热决策。抽成纯函数（只读那两张表）方便离线测试；调用方负责随后刷新 `LAST_ACTIVE`。
fn warm_decision(qq: i64) -> WarmDecision {
    // 距上次活跃超过 SESSION_GAP ＝ 新的时段开始了，必须重新预热。
    let same_window = LAST_ACTIVE
        .get(&qq)
        .is_some_and(|t| t.value().elapsed() <= SESSION_GAP);
    if !same_window {
        return WarmDecision::Warm;
    }
    match WARM_STATE.get(&qq).map(|v| *v.value()) {
        Some((at, true)) if at.elapsed() < WARM_MAX_AGE => WarmDecision::Reuse,
        Some((at, false)) if at.elapsed() < WARM_RETRY => WarmDecision::Skip,
        _ => WarmDecision::Warm,
    }
}

/// 取该用户的会话：优先用预热好的缓存，缓存没有才走完整恢复。
///
/// `get_client_or_err_for_id` 每次都会打一条 `profile` 验活，这在时段开始时是必要的，
/// 但进行中每轮再打一遍就是纯浪费——预热已经确认过了，这里直接用缓存，
/// 万一会话中途失效，调用方拿到请求错误后会自己回退到完整恢复。
fn warm_client(qq: i64) -> Option<SessionClient> {
    get_client_from_cache(qq)
}

async fn run_scouts(scouts: &[i64]) -> Vec<ScoutOutcome> {
    let mut tasks = Vec::with_capacity(scouts.len());
    for &qq in scouts {
        tasks.push(async move {
            // 简单随机把哨兵的轮询摊开，避免每轮在同一时刻齐射。
            tokio::time::sleep(Duration::from_secs(uniform(0..SCOUT_SPREAD_SECS))).await;

            // 预热过的缓存会话直接用：命中时本轮该哨兵只发 rollcalls 这一条请求。
            if let Some(client) = warm_client(qq)
                && let Ok(data) = Rollcalls::get_from_client(&client).await
            {
                return ScoutOutcome::Ok {
                    qq,
                    client,
                    rollcalls: data.rollcalls,
                };
            }

            // 缓存会话没有或中途失效，退回完整恢复再试一次。
            let client = match get_client_or_err_for_id(qq).await {
                Ok(client) => client,
                Err(e) => {
                    debug!(qq, error = ?e, "哨兵会话恢复失败");
                    WARM_STATE.insert(qq, (Instant::now(), false));
                    return ScoutOutcome::NotLogin { qq };
                }
            };
            match Rollcalls::get_from_client(&client).await {
                Ok(data) => ScoutOutcome::Ok {
                    qq,
                    client,
                    rollcalls: data.rollcalls,
                },
                Err(e) => {
                    warn!(qq, error = ?e, "哨兵轮询签到失败");
                    ScoutOutcome::Failed { qq }
                }
            }
        });
    }
    futures::future::join_all(tasks).await
}

async fn sign_one(qq: i64, rollcall_id: i64) -> SignOutcome {
    // 同样先用预热好的缓存会话，省掉一条验活请求。
    if let Some(client) = warm_client(qq) {
        match spec_sign_request_inner(qq, client.clone(), rollcall_id).await {
            Ok(responses) => return SignOutcome::Done { qq, responses },
            Err(e) => {
                // 失败原因分两种，必须分清：会话真的死了才值得换会话重来一遍。
                // 若会话还活着（雷达四种策略都没成、网络抖了一下），这次签到本身就是失败的，
                // 换个会话重跑等于把 answer PUT 再发一次——而那个 PUT 一发就是提交动作。
                if ProfileWithoutCache::get_from_client(&client).await.is_ok() {
                    warn!(qq, rollcall_id, error = ?e, "定时签到失败（会话正常，不重试）");
                    return SignOutcome::Failed;
                }
                debug!(qq, error = ?e, "缓存会话已失效，改用完整恢复重试一次");
            }
        }
    }

    let client = match get_client_or_err_for_id(qq).await {
        Ok(client) => client,
        Err(e) => {
            debug!(qq, error = ?e, "定时签到会话恢复失败");
            WARM_STATE.insert(qq, (Instant::now(), false));
            return SignOutcome::NotLogin { qq };
        }
    };
    match spec_sign_request_inner(qq, client, rollcall_id).await {
        Ok(responses) => SignOutcome::Done { qq, responses },
        Err(e) => {
            warn!(qq, rollcall_id, error = ?e, "定时签到失败");
            SignOutcome::Failed
        }
    }
}

fn absorb_sign(
    outcome: SignOutcome,
    rollcall_id: i64,
    reports: &mut HashMap<i64, Vec<AutoSignResponse>>,
    notlogin: &mut HashSet<i64>,
) {
    let (qq, responses) = match outcome {
        SignOutcome::NotLogin { qq } => {
            notlogin.insert(qq);
            return;
        }
        SignOutcome::Failed => return,
        SignOutcome::Done { qq, responses } => (qq, responses),
    };

    NOTLOGIN_REMINDED.remove(&qq);
    if responses.is_empty() {
        // 这个人的签到列表里根本没有这场签到：索引说他选了这门课，实际没有（退课 / 索引落后）。
        // 之后也不会突然冒出来，直接记账，免得每轮都为他白跑一次会话探测 + 列表查询。
        SIGNED_MARK.insert((qq, rollcall_id), Instant::now());
        return;
    }
    let mut settled = false;
    for response in responses {
        let (done, report) = classify(&response);
        settled |= done;
        if report {
            reports.entry(qq).or_default().push(response);
        }
    }
    if settled {
        SIGNED_MARK.insert((qq, rollcall_id), Instant::now());
    }
}

/// 判定一条签到结果：`(是否可以记账不再重试, 是否需要播报)`。
///
/// 只有服务端二次确认真的变成「已签到」才记账；二次确认没拿到就当没落地，
/// 下一轮还会再试一次，行为上不比改动前更冒进。
fn classify(response: &AutoSignResponse) -> (bool, bool) {
    match response {
        AutoSignResponse::Number(NumberSign::Success(data)) => {
            (data.second_check == Some(RollcallStatus::OnCallFine), true)
        }
        AutoSignResponse::Number(NumberSign::AlreadySigned(_)) => (true, false),
        AutoSignResponse::Radar(RadarSign::Success(data)) => {
            (data.second_check == Some(RollcallStatus::OnCallFine), true)
        }
        AutoSignResponse::Radar(RadarSign::AlreadySigned(_)) => (true, false),
        AutoSignResponse::Qr(QRSign::Success(_)) => (true, true),
        AutoSignResponse::Qr(QRSign::AlreadySigned(_)) => (true, false),
        AutoSignResponse::Qr(QRSign::Pending(_)) => (false, false),
    }
}

fn is_signed(qq: i64, rollcall_id: i64) -> bool {
    SIGNED_MARK
        .get(&(qq, rollcall_id))
        .is_some_and(|t| t.value().elapsed() < SIGNED_MARK_TTL)
}

async fn progress(client: &SessionClient, rollcall_id: i64) -> Option<(usize, usize)> {
    if let Some(entry) = PROGRESS_CACHE.get(&rollcall_id) {
        let (at, sign_num, student_num) = *entry.value();
        if at.elapsed() < PROGRESS_TTL {
            return Some((sign_num, student_num));
        }
    }
    match get_on_call_total_num(client, rollcall_id).await {
        Ok((sign_num, student_num)) => {
            PROGRESS_CACHE.insert(rollcall_id, (Instant::now(), sign_num, student_num));
            Some((sign_num, student_num))
        }
        Err(e) => {
            debug!(rollcall_id, error = ?e, "查询签到进度失败");
            None
        }
    }
}

fn prune_caches() {
    SIGNED_MARK.retain(|_, at| at.elapsed() < SIGNED_MARK_TTL);
    PROGRESS_CACHE.retain(|_, v| v.0.elapsed() < PROGRESS_TTL);
    HEAL_TRIED.retain(|_, at| at.elapsed() < HEAL_COOLDOWN);
    // 早就不活跃的人：清掉活跃/预热记录，下次上课自然会被当成新时段重新预热；
    // 当值记录也一并清掉，免得他隔天回来时因为“上次当班的轮次号很旧”被连着抓壮丁。
    LAST_ACTIVE.retain(|_, at| at.elapsed() < SIGNED_MARK_TTL);
    WARM_STATE.retain(|qq, _| LAST_ACTIVE.contains_key(qq));
    SCOUT_DUTY.retain(|qq, _| LAST_ACTIVE.contains_key(qq));
}

async fn send_reports(
    reports: HashMap<i64, Vec<AutoSignResponse>>,
    notlogin: HashSet<i64>,
    groups: &HashMap<i64, i64>,
) {
    let client = get_client();
    let mut tasks = Vec::new();

    for (qq, response) in reports {
        if response.is_empty() {
            continue;
        }
        let Some(&group_id) = groups.get(&qq) else {
            continue;
        };
        let params = SendGroupMessageParams {
            group_id,
            message: Arc::new(
                MessageSend::new_message()
                    .at(qq.to_string())
                    .text(format!(
                        "定时签到结果:\n{}",
                        response
                            .iter()
                            .map(|r| format!("{}\n", r))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ))
                    .build(),
            ),
        };
        trace!(qq = qq, group_id = group_id, params = ?params, "准备发送定时签到消息");
        tasks.push(send_one(client, params));
    }

    for qq in notlogin {
        // 登录失效：保留课表，仅提醒一次（DashSet::insert 返回 true 表示首次）。
        if !NOTLOGIN_REMINDED.insert(qq) {
            continue;
        }
        let Some(&group_id) = groups.get(&qq) else {
            continue;
        };
        let params = SendGroupMessageParams {
            group_id,
            message: Arc::new(
                MessageSend::new_message()
                    .at(qq.to_string())
                    .text(
                        "定时签到检测到登录已失效，请重新登录（课程表已保留，重新登录后会自动恢复定时签到）",
                    )
                    .build(),
            ),
        };
        trace!(qq = qq, group_id = group_id, params = ?params, "准备发送登录失效提醒");
        tasks.push(send_one(client, params));
    }

    futures::future::join_all(tasks).await;
}

async fn send_one<C: BotClient + Send + Sync + 'static>(
    client: &'static Arc<C>,
    params: SendGroupMessageParams,
) {
    let echo = match client.call_api(&params, Echo::new()).await {
        Ok(echo) => echo,
        Err(e) => {
            error!("定时签到消息发送失败: {:?}", e);
            return;
        }
    };
    match echo.wait_echo().await {
        Ok(e) => info!("定时签到消息发送成功: {:?}", e),
        Err(e) => error!("定时签到消息发送失败: {:?}", e),
    }
}

pub static TIME_SIGN_TASK_RUNNER: LazyLock<Arc<TaskRunner<TimeSignTask>>> =
    LazyLock::new(|| TaskRunner::new(TimeSignTask));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::rollcall::auto_sign_data::RadarType;

    fn radar_success(second_check: Option<RollcallStatus>) -> AutoSignResponse {
        AutoSignResponse::radar_success(
            "课".into(),
            "教室".into(),
            0.0,
            0.0,
            1.0,
            RadarType::Cache,
            second_check,
        )
    }

    #[test]
    fn classify_marks_only_confirmed_signs() {
        // 二次确认拿到「已签到」才记账
        assert_eq!(
            classify(&radar_success(Some(RollcallStatus::OnCallFine))),
            (true, true)
        );
        // 二次确认没拿到 / 还是缺勤：照旧播报成功，但下一轮还会再试
        assert_eq!(classify(&radar_success(None)), (false, true));
        assert_eq!(
            classify(&radar_success(Some(RollcallStatus::Absent))),
            (false, true)
        );
        // 已经签过：记账、但不再刷屏
        assert_eq!(
            classify(&AutoSignResponse::radar_already_signed("课".into())),
            (true, false)
        );
        assert_eq!(
            classify(&AutoSignResponse::number_already_signed("课".into())),
            (true, false)
        );
        // 二维码只能等人发码，不记账也不播报
        assert_eq!(
            classify(&AutoSignResponse::qr_pending("课".into())),
            (false, false)
        );
    }

    #[test]
    fn signed_mark_suppresses_further_attempts() {
        let qq = -9001;
        let rollcall_id = -9001;
        assert!(!is_signed(qq, rollcall_id));

        let mut reports = HashMap::new();
        let mut notlogin = HashSet::new();
        absorb_sign(
            SignOutcome::Done {
                qq,
                responses: vec![radar_success(Some(RollcallStatus::OnCallFine))],
            },
            rollcall_id,
            &mut reports,
            &mut notlogin,
        );

        assert!(is_signed(qq, rollcall_id), "确认落地后应当记账，不再重试");
        assert_eq!(reports[&qq].len(), 1, "成功要播报一次");
        assert!(notlogin.is_empty());
        SIGNED_MARK.remove(&(qq, rollcall_id));
    }

    #[test]
    fn unconfirmed_sign_is_retried_next_tick() {
        let qq = -9002;
        let rollcall_id = -9002;
        let mut reports = HashMap::new();
        let mut notlogin = HashSet::new();
        absorb_sign(
            SignOutcome::Done {
                qq,
                responses: vec![radar_success(None)],
            },
            rollcall_id,
            &mut reports,
            &mut notlogin,
        );

        assert!(!is_signed(qq, rollcall_id), "二次确认没拿到就不该记账");
        assert_eq!(reports[&qq].len(), 1);
    }

    #[test]
    fn already_signed_is_marked_but_not_reported() {
        let qq = -9003;
        let rollcall_id = -9003;
        let mut reports = HashMap::new();
        let mut notlogin = HashSet::new();
        absorb_sign(
            SignOutcome::Done {
                qq,
                responses: vec![AutoSignResponse::radar_already_signed("课".into())],
            },
            rollcall_id,
            &mut reports,
            &mut notlogin,
        );

        assert!(is_signed(qq, rollcall_id));
        assert!(reports.is_empty(), "已签到不该再刷一条消息");
        SIGNED_MARK.remove(&(qq, rollcall_id));
    }

    #[test]
    fn empty_response_is_marked_to_stop_retrying() {
        // 索引说他选了这门课、实际列表里没有这场签到：记账，别每轮再白跑一次。
        let qq = -9007;
        let rollcall_id = -9007;
        let mut reports = HashMap::new();
        let mut notlogin = HashSet::new();
        absorb_sign(
            SignOutcome::Done {
                qq,
                responses: vec![],
            },
            rollcall_id,
            &mut reports,
            &mut notlogin,
        );

        assert!(is_signed(qq, rollcall_id));
        assert!(reports.is_empty());
        SIGNED_MARK.remove(&(qq, rollcall_id));
    }

    #[test]
    fn notlogin_outcome_is_collected() {
        let qq = -9004;
        let mut reports = HashMap::new();
        let mut notlogin = HashSet::new();
        absorb_sign(
            SignOutcome::NotLogin { qq },
            -9004,
            &mut reports,
            &mut notlogin,
        );
        assert!(notlogin.contains(&qq));
        assert!(reports.is_empty());
    }

    #[test]
    fn backups_cover_courses_left_blind_by_failed_scouts() {
        let plan = TickPlan {
            scouts: vec![1],
            members: HashMap::from([(10, vec![1, 2, 3]), (20, vec![1, 4])]),
        };
        let courses_of = HashMap::from([
            (1, vec![10, 20]),
            (2, vec![10]),
            (3, vec![10]),
            (4, vec![20]),
        ]);
        let mut gathered = Gathered::default();
        gathered.failed.insert(1);

        let backups = pick_backups(&plan, &courses_of, &gathered);
        assert_eq!(backups.len(), 2, "两门课各补一个人：{:?}", backups);
        assert!(!backups.contains(&1), "失败的哨兵不该被再选一次");
    }

    #[test]
    fn no_backup_when_everything_is_covered() {
        let plan = TickPlan {
            scouts: vec![1],
            members: HashMap::from([(10, vec![1, 2])]),
        };
        let courses_of = HashMap::from([(1, vec![10]), (2, vec![10])]);
        let mut gathered = Gathered::default();
        gathered.covered.insert(10);

        assert!(pick_backups(&plan, &courses_of, &gathered).is_empty());
    }

    #[test]
    fn one_backup_can_cover_several_blind_courses() {
        let plan = TickPlan {
            scouts: vec![1],
            members: HashMap::from([(10, vec![1, 2]), (20, vec![1, 2])]),
        };
        let courses_of = HashMap::from([(1, vec![10, 20]), (2, vec![10, 20])]);
        let mut gathered = Gathered::default();
        gathered.failed.insert(1);

        assert_eq!(pick_backups(&plan, &courses_of, &gathered), vec![2]);
    }

    #[test]
    fn scout_success_clears_notlogin_reminder() {
        let qq = -9005;
        NOTLOGIN_REMINDED.insert(qq);
        let mut gathered = Gathered::default();
        gathered.absorb(
            vec![ScoutOutcome::Ok {
                qq,
                client: SessionClient::new(),
                rollcalls: vec![],
            }],
            &HashMap::from([(qq, vec![10])]),
            &HashMap::from([(qq, vec![10])]),
        );
        assert!(!NOTLOGIN_REMINDED.contains(&qq));
        assert!(gathered.covered.contains(&10));
    }

    #[test]
    fn qr_rollcalls_are_not_watched() {
        let mut gathered = Gathered::default();
        gathered.absorb(
            vec![ScoutOutcome::Ok {
                qq: -9006,
                client: SessionClient::new(),
                rollcalls: vec![
                    Rollcall {
                        course_id: 1,
                        course_title: "二维码".into(),
                        is_number: false,
                        is_radar: false,
                        rollcall_id: 100,
                        status: RollcallStatus::Absent,
                    },
                    Rollcall {
                        course_id: 2,
                        course_title: "雷达".into(),
                        is_number: false,
                        is_radar: true,
                        rollcall_id: 200,
                        status: RollcallStatus::Absent,
                    },
                ],
            }],
            &HashMap::from([(-9006, vec![1, 2])]),
            &HashMap::from([(-9006, vec![1, 2])]),
        );
        assert!(!gathered.found.contains_key(&100), "二维码签到不进入派发");
        assert!(gathered.found.contains_key(&200));
    }

    /// 时段开始必须试一次（提醒时机不变），时段进行中不再重复验活（这才是要省的）。
    #[test]
    fn warm_up_runs_once_per_class_window() {
        let qq = -9101;
        LAST_ACTIVE.remove(&qq);
        WARM_STATE.remove(&qq);

        // 从没见过这个人：时段开始，必须预热
        assert_eq!(warm_decision(qq), WarmDecision::Warm);

        // 预热成功后，同一时段里的后续轮次直接沿用，不再发验活请求
        LAST_ACTIVE.insert(qq, Instant::now());
        WARM_STATE.insert(qq, (Instant::now(), true));
        assert_eq!(warm_decision(qq), WarmDecision::Reuse);

        // 隔了一个 SESSION_GAP 没活跃 ＝ 新的时段，重新预热
        LAST_ACTIVE.insert(qq, Instant::now() - SESSION_GAP - Duration::from_secs(1));
        assert_eq!(warm_decision(qq), WarmDecision::Warm);

        LAST_ACTIVE.remove(&qq);
        WARM_STATE.remove(&qq);
    }

    /// 预热失败的人：本时段不再骚扰，但过了重试点还要捞一次（他可能自己重新登录了）。
    #[test]
    fn failed_warm_up_is_skipped_then_retried() {
        let qq = -9102;
        LAST_ACTIVE.insert(qq, Instant::now());

        WARM_STATE.insert(qq, (Instant::now(), false));
        assert_eq!(warm_decision(qq), WarmDecision::Skip);

        WARM_STATE.insert(
            qq,
            (Instant::now() - WARM_RETRY - Duration::from_secs(1), false),
        );
        assert_eq!(warm_decision(qq), WarmDecision::Warm);

        LAST_ACTIVE.remove(&qq);
        WARM_STATE.remove(&qq);
    }

    /// 连堂课会把活跃窗口连成一大片，这时也不能一直吃老结论。
    #[test]
    fn stale_warm_result_is_refreshed_even_inside_one_window() {
        let qq = -9103;
        LAST_ACTIVE.insert(qq, Instant::now());
        WARM_STATE.insert(
            qq,
            (Instant::now() - WARM_MAX_AGE - Duration::from_secs(1), true),
        );

        assert_eq!(warm_decision(qq), WarmDecision::Warm);

        LAST_ACTIVE.remove(&qq);
        WARM_STATE.remove(&qq);
    }

    /// 当值记录真的会被写进去，下一轮排班才轮得动。
    #[test]
    fn scout_duty_is_recorded_for_rotation() {
        let a = -9104;
        let b = -9105;
        SCOUT_DUTY.remove(&a);
        SCOUT_DUTY.remove(&b);

        SCOUT_DUTY.insert(a, 5);
        let last_duty: HashMap<i64, u64> = [a, b]
            .iter()
            .filter_map(|&qq| SCOUT_DUTY.get(&qq).map(|v| (qq, *v.value())))
            .collect();
        assert_eq!(last_duty.get(&a), Some(&5));
        assert_eq!(
            last_duty.get(&b),
            None,
            "没当过班的人查不到记录，排班里当作 0"
        );

        // 上次第 5 轮当过班的 a 要给从没当过的 b 让位
        let courses_of = HashMap::from([(a, vec![1, 2]), (b, vec![1, 2])]);
        let plan = crate::logic::rollcall::watch::plan_tick(&[a, b], &courses_of, &last_duty);
        assert_eq!(plan.scouts, vec![b]);

        SCOUT_DUTY.remove(&a);
        SCOUT_DUTY.remove(&b);
    }

    #[test]
    fn failed_scout_is_recorded() {
        let mut gathered = Gathered::default();
        gathered.absorb(
            vec![
                ScoutOutcome::Failed { qq: 1 },
                ScoutOutcome::NotLogin { qq: 2 },
            ],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(gathered.failed.contains(&1));
        assert!(gathered.failed.contains(&2));
        assert!(gathered.notlogin.contains(&2));
        assert!(!gathered.notlogin.contains(&1));
    }
}
