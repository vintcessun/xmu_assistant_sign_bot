//! 定时签到的“同课分组蹲守”排班。
//!
//! 原本每个处于上课时间段的用户，每轮都自己打一次 `radar/rollcalls`；同一门课上有几个人
//! 就重复几遍完全相同的查询。但 `radar/rollcalls` 是**按人**返回“他选的所有课的签到”，
//! 所以只要有一个人去查，这门课上的签到就已经被发现了。
//!
//! 于是把本轮活跃用户按选课分组，挑出一小批“哨兵(scout)”，让他们的选课并集盖住所有
//! 需要盯的课；其余人本轮一个请求都不发，直到哨兵真的发现签到才被派发去签。
//!
//! 排班的第一优先级是**轮值**而不是最小哨兵数：每轮从「最久没当过哨兵的人」开始挑，
//! 只有当值资历相同时才比谁能一次盖住更多课。纯按覆盖数贪心的话，选课最多的人会**每轮**
//! 都被选中——总请求是降下来了，但全压在少数几个人头上，其余人一次都轮不到。
//! 轮值让每个人的请求量都摊薄成大约「哨兵数 / 活跃人数」，代价只是偶尔多一个哨兵。
//!
//! 这里只做**纯计算**，不碰网络，方便离线测试。

use super::utils::uniform;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

/// 一轮蹲守的分工。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickPlan {
    /// 本轮真正去打 `radar/rollcalls` 的用户。
    pub scouts: Vec<i64>,
    /// `course_id -> 本轮活跃且选了该课的用户`，哨兵发现签到后按这张表派发。
    pub members: HashMap<i64, Vec<i64>>,
}

/// 挑哨兵的比较键：`(Reverse(上次当值轮次), 本轮能覆盖的新课数, 随机决胜)`。
/// `Reverse` 让「最久没当过班」排在最前，覆盖数只在资历相同时才说得上话。
type DutyKey = (Reverse<u64>, usize, u64);

/// 轮值优先的覆盖排班。
///
/// * `active`：本轮处于「课程时间段前后十分钟」且会话可用的用户；
/// * `courses_of`：**只**包含选课索引里查得到的活跃用户。索引里查不到的人
///   （刚登录还没并入、拉课失败）一律自己当哨兵，否则他们的课没人盯就会漏签；
/// * `last_duty`：`qq -> 上次当哨兵的轮次号`，没当过的当作 0（最优先）。
///
/// 索引整体不可用时 `courses_of` 为空，于是 `scouts == active`，退化成改动前的逐人轮询。
pub fn plan_tick(
    active: &[i64],
    courses_of: &HashMap<i64, Vec<i64>>,
    last_duty: &HashMap<i64, u64>,
) -> TickPlan {
    let mut scouts = Vec::new();
    let mut members: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut known: Vec<i64> = Vec::new();

    for &qq in active {
        match courses_of.get(&qq) {
            Some(courses) if !courses.is_empty() => {
                known.push(qq);
                for &c in courses {
                    members.entry(c).or_default().push(qq);
                }
            }
            _ => scouts.push(qq),
        }
    }

    let mut uncovered: HashSet<i64> = members.keys().copied().collect();
    // 每个候选带两个权重：上次当值的轮次号（越小＝越久没当，越该轮到他），
    // 以及一个随机数用于同资历时打散，避免长期固定某个顺序。
    let mut candidates: Vec<(i64, u64, u64)> = known
        .into_iter()
        .map(|qq| {
            (
                qq,
                last_duty.get(&qq).copied().unwrap_or(0),
                uniform(0..u64::MAX),
            )
        })
        .collect();

    while !uncovered.is_empty() {
        // 比较键：Reverse(当值轮次) 优先 —— 最久没当过的先上；资历相同再比这一轮能盖住
        // 多少门未覆盖的课，最后才用随机数决胜。
        let mut best: Option<(usize, DutyKey)> = None;
        for (i, (qq, duty, tie)) in candidates.iter().enumerate() {
            let gain = courses_of[qq]
                .iter()
                .filter(|c| uncovered.contains(*c))
                .count();
            if gain == 0 {
                continue;
            }
            let key = (Reverse(*duty), gain, *tie);
            if best.as_ref().is_none_or(|(_, best_key)| key > *best_key) {
                best = Some((i, key));
            }
        }
        // uncovered 里的课都来自某个候选，正常不会取不到；取不到就直接收尾避免死循环。
        let Some((idx, _)) = best else {
            break;
        };
        let (qq, _, _) = candidates.swap_remove(idx);
        for c in &courses_of[&qq] {
            uncovered.remove(c);
        }
        scouts.push(qq);
    }

    TickPlan { scouts, members }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn courses(pairs: &[(i64, &[i64])]) -> HashMap<i64, Vec<i64>> {
        pairs.iter().map(|(qq, cs)| (*qq, cs.to_vec())).collect()
    }

    /// 谁都没当过班时的排班（`last_duty` 为空 ＝ 全员资历相同，覆盖数说了算）。
    fn plan_fresh(active: &[i64], courses_of: &HashMap<i64, Vec<i64>>) -> TickPlan {
        plan_tick(active, courses_of, &HashMap::new())
    }

    /// 排班必须盖住所有需要盯的课，否则会漏签。
    fn assert_covers(plan: &TickPlan, courses_of: &HashMap<i64, Vec<i64>>) {
        let covered: HashSet<i64> = plan
            .scouts
            .iter()
            .filter_map(|qq| courses_of.get(qq))
            .flatten()
            .copied()
            .collect();
        for c in plan.members.keys() {
            assert!(covered.contains(c), "课程 {c} 本轮没有哨兵盯着");
        }
    }

    /// 跑一整节课的轮次，模拟真实调度：当过班就记下轮次号。
    fn simulate(
        active: &[i64],
        courses_of: &HashMap<i64, Vec<i64>>,
        ticks: u64,
    ) -> HashMap<i64, u64> {
        let mut last_duty: HashMap<i64, u64> = HashMap::new();
        let mut duty_count: HashMap<i64, u64> = active.iter().map(|&qq| (qq, 0)).collect();
        for tick in 1..=ticks {
            let plan = plan_tick(active, courses_of, &last_duty);
            for qq in plan.scouts {
                last_duty.insert(qq, tick);
                *duty_count.entry(qq).or_default() += 1;
            }
        }
        duty_count
    }

    #[test]
    fn one_scout_is_enough_when_everyone_shares_the_same_course() {
        let active = vec![1, 2, 3, 4, 5];
        let courses_of = courses(&[
            (1, &[100]),
            (2, &[100]),
            (3, &[100]),
            (4, &[100]),
            (5, &[100]),
        ]);
        let plan = plan_fresh(&active, &courses_of);

        assert_eq!(plan.scouts.len(), 1, "同一门课 5 个人只需要 1 个哨兵");
        assert_covers(&plan, &courses_of);
        let mut m = plan.members[&100].clone();
        m.sort_unstable();
        assert_eq!(m, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn disjoint_courses_force_everyone_to_scout() {
        let active = vec![1, 2, 3];
        let courses_of = courses(&[(1, &[10]), (2, &[20]), (3, &[30])]);
        let plan = plan_fresh(&active, &courses_of);

        assert_eq!(plan.scouts.len(), 3);
        assert_covers(&plan, &courses_of);
    }

    #[test]
    fn a_superset_user_covers_everyone_else() {
        let active = vec![1, 2, 3, 4];
        let courses_of = courses(&[(1, &[10, 20, 30]), (2, &[10]), (3, &[20]), (4, &[30])]);
        let plan = plan_fresh(&active, &courses_of);

        assert_eq!(
            plan.scouts,
            vec![1],
            "同资历时选课最全的人一个人就能盖住全部课程"
        );
        assert_covers(&plan, &courses_of);
    }

    /// 轮值优先于覆盖数：刚当过班的“超集用户”本轮要让位，哪怕他一个人就能全包。
    #[test]
    fn duty_rotation_outranks_coverage() {
        let active = vec![1, 2, 3, 4];
        let courses_of = courses(&[(1, &[10, 20, 30]), (2, &[10]), (3, &[20]), (4, &[30])]);
        let last_duty = HashMap::from([(1, 7u64)]);

        let plan = plan_tick(&active, &courses_of, &last_duty);
        assert!(
            !plan.scouts.contains(&1),
            "刚当过班的人不该连任：{:?}",
            plan.scouts
        );
        assert_covers(&plan, &courses_of);
    }

    #[test]
    fn users_missing_from_index_scout_for_themselves() {
        let active = vec![1, 2, 7];
        // 7 不在索引里（刚登录 / 拉课失败）
        let courses_of = courses(&[(1, &[10]), (2, &[10])]);
        let plan = plan_fresh(&active, &courses_of);

        assert!(plan.scouts.contains(&7), "索引查不到的人必须自己当哨兵");
        assert_eq!(plan.scouts.len(), 2, "10 号课一个哨兵 + 兜底的 7");
        assert_covers(&plan, &courses_of);
        // 索引不知道 7 选了什么，所以 members 里不会有他
        assert!(!plan.members.values().any(|v| v.contains(&7)));
    }

    #[test]
    fn empty_index_degrades_to_per_user_polling() {
        let active = vec![1, 2, 3];
        let plan = plan_fresh(&active, &HashMap::new());

        let mut scouts = plan.scouts.clone();
        scouts.sort_unstable();
        assert_eq!(scouts, active, "没有索引时退化成改动前的逐人轮询");
        assert!(plan.members.is_empty());
    }

    #[test]
    fn empty_course_list_is_treated_as_unknown() {
        let active = vec![1];
        let courses_of = courses(&[(1, &[])]);
        let plan = plan_fresh(&active, &courses_of);
        assert_eq!(plan.scouts, vec![1]);
    }

    #[test]
    fn no_active_user_means_no_work() {
        let plan = plan_fresh(&[], &HashMap::new());
        assert!(plan.scouts.is_empty());
        assert!(plan.members.is_empty());
    }

    #[test]
    fn overlapping_groups_pick_a_small_cover() {
        let active: Vec<i64> = (1..=12).collect();
        // 12 个人分布在 4 门课上，每人 2 门，两两交叠
        let courses_of = courses(&[
            (1, &[10, 20]),
            (2, &[10, 20]),
            (3, &[10, 20]),
            (4, &[20, 30]),
            (5, &[20, 30]),
            (6, &[20, 30]),
            (7, &[30, 40]),
            (8, &[30, 40]),
            (9, &[30, 40]),
            (10, &[40, 10]),
            (11, &[40, 10]),
            (12, &[40, 10]),
        ]);
        let plan = plan_fresh(&active, &courses_of);

        assert_covers(&plan, &courses_of);
        assert!(
            plan.scouts.len() <= 2,
            "每人 2 门课、共 4 门，2 个哨兵就够，实际 {}",
            plan.scouts.len()
        );
        assert_eq!(plan.members.len(), 4);
        // 每门课上有两组共 6 个人，派发时这 6 个人都会被带上
        for c in [10, 20, 30, 40] {
            assert_eq!(plan.members[&c].len(), 6);
        }
    }

    #[test]
    fn members_only_contain_active_users() {
        let active = vec![1, 2];
        // 3 选了同一门课但本轮不在上课时间段，调用方不会把他放进 courses_of
        let courses_of = courses(&[(1, &[10]), (2, &[10])]);
        let plan = plan_fresh(&active, &courses_of);

        assert!(!plan.members[&10].contains(&3));
    }

    /// 关键不变量：改动前每个活跃用户都会看到自己每一门课的签到；
    /// 改动后必须仍然能通过「哨兵发现 + 按课派发」覆盖到同样的 (用户, 课) 组合，
    /// 否则就是在悄悄漏签。这里用一堆构造出来的选课分布反复验证。
    #[test]
    fn every_active_member_is_still_reachable() {
        for seed in 0..50i64 {
            let user_num = 3 + seed % 10;
            let active: Vec<i64> = (1..=user_num).collect();
            let mut courses_of: HashMap<i64, Vec<i64>> = HashMap::new();
            for &qq in &active {
                let mut cs: Vec<i64> = (0..3)
                    .map(|k| ((qq * 7 + k * 13 + seed) % 5) * 10)
                    .collect();
                cs.sort_unstable();
                cs.dedup();
                courses_of.insert(qq, cs);
            }
            // 带上一份乱七八糟的当值历史，确认轮值不会破坏覆盖完整性
            let last_duty: HashMap<i64, u64> = active
                .iter()
                .map(|&qq| (qq, (qq * 3 + seed) as u64 % 7))
                .collect();

            let plan = plan_tick(&active, &courses_of, &last_duty);
            let covered: HashSet<i64> = plan
                .scouts
                .iter()
                .filter_map(|qq| courses_of.get(qq))
                .flatten()
                .copied()
                .collect();

            for (&qq, cs) in &courses_of {
                for c in cs {
                    assert!(covered.contains(c), "seed={seed} 课程 {c} 本轮没有哨兵");
                    assert!(
                        plan.members[c].contains(&qq),
                        "seed={seed} 用户 {qq} 不在课程 {c} 的派发名单里"
                    );
                }
            }
        }
    }

    /// 收益的量化：一节大课的规模下，真正发轮询请求的人应当只剩个位数。
    #[test]
    fn scouts_are_far_fewer_than_active_users() {
        let active: Vec<i64> = (1..=40).collect();
        // 40 个人分布在 12 门课上，人均 6 门（连续区间，交叠很多）
        let courses_of: HashMap<i64, Vec<i64>> = active
            .iter()
            .map(|&qq| (qq, (0..6).map(|k| (qq + k) % 12 * 10).collect::<Vec<_>>()))
            .collect();

        let plan = plan_fresh(&active, &courses_of);
        assert_covers(&plan, &courses_of);
        assert!(
            plan.scouts.len() <= 4,
            "40 人 12 门课应当只需要个位数哨兵，实际 {}",
            plan.scouts.len()
        );
    }

    /// 本次改动的核心诉求：**每个人**的请求量都要降下来，而不是压在少数人头上。
    #[test]
    fn duty_is_shared_evenly_across_a_whole_class() {
        let active: Vec<i64> = (1..=30).collect();
        // 30 个人、12 门课、人均 6 门
        let courses_of: HashMap<i64, Vec<i64>> = active
            .iter()
            .map(|&qq| (qq, (0..6).map(|k| (qq + k) % 12 * 10).collect::<Vec<_>>()))
            .collect();

        // 一节课的时间窗（前后各十分钟）大约几十轮，这里取 60 轮
        let ticks = 60;
        let duty = simulate(&active, &courses_of, ticks);

        let min = *duty.values().min().unwrap();
        let max = *duty.values().max().unwrap();
        let total: u64 = duty.values().sum();
        println!(
            "{ticks} 轮 / {} 人：总当班 {total} 次，人均 {:.1}，最少 {min}，最多 {max}（改动前是每人 {ticks} 次）",
            active.len(),
            total as f64 / active.len() as f64
        );

        // 谁都不能一次都不当班（否则就是“很多人没被用到”），
        // 也不能有人被反复抓壮丁：最忙的人不超过最闲的人的两倍再加一。
        assert!(min > 0, "有人整节课一次都没轮到：{duty:?}");
        assert!(
            max <= 2 * min + 1,
            "当班次数分布不均：min={min} max={max} {duty:?}"
        );
        // 总当班次数 ＝ 总轮询请求数，应当远小于“每人每轮都查”的 30*60
        assert!(
            total < (active.len() as u64 * ticks) / 5,
            "总轮询次数没降下来：{total}"
        );
        // 单个人自己的请求数也要降到原来的零头（原来是每轮 1 次 ＝ 60 次）
        assert!(max * 4 < ticks, "单人请求没降下来：max={max} / {ticks} 轮");
    }

    /// 最容易“抓壮丁”的形状：1 号一个人盖住所有课，纯覆盖数贪心会让他每轮连任。
    #[test]
    fn duty_rotates_even_when_one_user_covers_everything() {
        let active: Vec<i64> = (1..=6).collect();
        let courses_of = courses(&[
            (1, &[10, 20, 30]),
            (2, &[10]),
            (3, &[10, 20]),
            (4, &[20, 30]),
            (5, &[30]),
            (6, &[10, 30]),
        ]);

        let duty = simulate(&active, &courses_of, 60);
        assert!(
            duty[&1] < 30,
            "选课最全的人被反复抓壮丁：{} / 60 轮，{duty:?}",
            duty[&1]
        );
        assert!(duty.values().all(|&n| n > 0), "有人一次都没轮到：{duty:?}");
    }
}
