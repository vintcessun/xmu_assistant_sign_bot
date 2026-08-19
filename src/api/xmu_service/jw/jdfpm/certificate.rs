use super::{JDFPM_APP, JwAPI};
use crate::api::network::SessionClient;
use anyhow::{Result, anyhow, bail};
use bytes::Bytes;
use helper::castgc_client_helper;
use serde::Serialize;
use tracing::{debug, info, warn};

/// 绩点证明（`printZm.do`）返回的是 PDF 字节流而不是 JSON，
/// 所以这里不套 `jw_api` 宏，只手写实现同一个 `JwAPI` 契约（入口页 + 数据地址）。
pub struct GpaCertificate;

#[async_trait::async_trait]
impl JwAPI for GpaCertificate {
    const URL_DATA: &'static str = "https://jw.xmu.edu.cn/jwapp/sys/jdfpm/api/jdjs/printZm.do";
    const APP_ENTRANCE: &'static str = JDFPM_APP;
}

/// 打印内容（DYNR）：JD=绩点，JQPJF=加权平均分。取值与浏览器抓包一致。
pub const PRINT_CONTENT: &str = "JD,JQPJF";

/// 页眉页脚特征：含这些片段的行在提取正文时直接丢掉。
const NOISE_MARKERS: [&str; 4] = ["电话∶", "TEL:", "ADD:", "页 / 共"];
/// 附录（绩点换算表与计算公式）的起始标记，正文提取到此为止。
const APPENDIX_MARKERS: [&str; 2] = ["附：", "附:"];

impl GpaCertificate {
    /// 下载指定计算结果（`GpaRecordDetail::wid`）的中文绩点证明 PDF。
    #[castgc_client_helper]
    pub async fn download_from_client(
        client: &SessionClient,
        wid: &str,
        show_rank: bool,
    ) -> Result<Bytes> {
        // 与 jw_api 生成的调用一致：先走应用入口换取 jwapp 会话。
        client.get(Self::APP_ENTRANCE).await?;

        let url = format!(
            "{}?wid={}&type=zw&DYNR={}&SFXYPM={}",
            Self::URL_DATA,
            urlencoding::encode(wid),
            urlencoding::encode(PRINT_CONTENT),
            u8::from(show_rank)
        );
        debug!(wid = wid, show_rank = show_rank, "开始下载绩点证明 PDF");

        let resp = client.get(&url).await?;
        let status = resp.status();
        if !status.is_success() {
            bail!("下载绩点证明失败，教务返回状态码 {status}");
        }

        let bytes = resp.bytes().await?;
        // 会话过期时教务会 200 返回统一身份认证的登录页 HTML，必须挡在解析之前。
        if !bytes.starts_with(b"%PDF") {
            warn!(wid = wid, len = bytes.len(), "printZm 返回的不是 PDF");
            bail!("教务返回的不是 PDF（登录会话可能已过期，请重新登录后重试）");
        }

        info!(wid = wid, len = bytes.len(), "绩点证明 PDF 下载完成");
        Ok(bytes)
    }

    /// 提取 PDF 正文。解析是纯 CPU 工作，放到阻塞线程池里跑，避免卡住运行时。
    pub async fn extract_text(pdf: Bytes) -> Result<String> {
        let text = tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&pdf))
            .await
            .map_err(|e| anyhow!("PDF 解析任务异常退出: {e}"))?
            .map_err(|e| anyhow!("解析绩点证明 PDF 失败: {e}"))?;

        let text = normalize_text(&text);
        if text.is_empty() {
            bail!("绩点证明 PDF 未能提取到文字（可能是扫描件或未内嵌 ToUnicode 字体）");
        }
        debug!(len = text.len(), "绩点证明 PDF 文本提取完成");
        Ok(text)
    }
}

/// 压掉 PDF 提取常见的空行与行尾空白，让网页展示更紧凑。
fn normalize_text(raw: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        // 连续空行只保留一行。
        if line.is_empty() && lines.last().is_none_or(|last: &&str| last.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// 从绩点证明正文里抽出的关键信息。
///
/// 排名、专业人数这些字段教务的 JSON 接口一律返回 `"*"`，只有证明 PDF 里才写明，
/// 所以这一步是整条链路的意义所在。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CertificateFacts {
    /// 证明主体：已去掉页眉页脚、合并硬换行、截掉附录。
    pub statement: String,
    /// 加权平均分（百分制）
    pub weighted_average: Option<String>,
    /// 平均学分绩点（4 分制）
    pub gpa: Option<String>,
    /// 所在专业总人数
    pub major_size: Option<String>,
    /// 专业绩点排名
    pub rank: Option<String>,
    /// 成绩数据截止日期
    pub deadline: Option<String>,
}

/// 解析证明正文。
///
/// PDF 的正文是按版面宽度硬换行的（真实样本里“绩点排名20”会被切成
/// “绩点排名2”和“0。成绩以…”两行），所以必须先把段落拼回连续文本再抽字段，
/// 逐行匹配是拿不到完整数字的。
pub fn extract_facts(text: &str) -> CertificateFacts {
    let body = APPENDIX_MARKERS
        .iter()
        .filter_map(|marker| text.find(marker))
        .min()
        .map_or(text, |idx| &text[..idx]);

    let statement = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !NOISE_MARKERS.iter().any(|noise| line.contains(noise)))
        .collect::<Vec<_>>()
        .join("");

    CertificateFacts {
        weighted_average: number_after(&statement, "加权平均分为"),
        gpa: number_after(&statement, "学分绩点为"),
        major_size: number_after(&statement, "专业总人数"),
        rank: number_after(&statement, "绩点排名"),
        deadline: between(&statement, "成绩以", "数据为准"),
        statement,
    }
}

/// 取 `marker` 之后紧跟的数字。
///
/// 会跳过后面不是数字的出现位置——标题“……学分绩点排名证明”里也有“绩点排名”，
/// 只取正文里那个真正带数字的。
fn number_after(text: &str, marker: &str) -> Option<String> {
    text.match_indices(marker).find_map(|(idx, _)| {
        let rest = &text[idx + marker.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        (end > 0).then(|| rest[..end].to_owned())
    })
}

fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let rest = &text[text.find(start)? + start.len()..];
    Some(rest[..rest.find(end)?].trim().to_owned())
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::testenv;
    use super::*;

    /// 真实证明 PDF 的提取结果（姓名/学号已脱敏，排版换行原样保留）。
    /// 注意“绩点排名20”被版面硬换行切成了“绩点排名2”和“0。成绩以…”。
    const SAMPLE: &str = "教务处 电话∶0592-2186251 传真∶0592-2096192 电子信箱∶JWC@XMU.EDU.CN    |  ACADEMIC AFFAIRS OFFICE TEL: +(86)592-2186251
地址∶福建省厦门市思明区思明南路422号 邮编∶361005                       |  ADD: XIAMEN UNIVERSITY, 361005 FUJIAN PROVINCE, P.R.CHINA

第1页 / 共1页

厦门大学本科学分绩点排名证明

姓名：某某某，学号：00000000000000，性别：男，信息学院软件工程
专业2024级在读本科生。根据我校绩点计算方法，该生2025-2026学年 第一
学期至2025-2026学年 第三学期 含校选课程加权平均分为87.91（百分
制），学分绩点为3.69（4分制）。该生所在专业总人数172人，绩点排名2
0。成绩以2026年08月14日数据为准。
特此证明。

附：厦门大学学分绩点计算方法

1.课程百分制成绩、课程绩点与等级换算关系表
95-100 A+ 4.0
平均学分绩点（GPA）=∑（课程绩点×课程学分）/∑课程学分
";

    #[test]
    fn normalize_collapses_blank_lines() {
        let raw = "  厦门大学  


绩点证明 

";
        assert_eq!(normalize_text(raw), "厦门大学

绩点证明");
    }

    #[test]
    fn extracts_facts_from_real_layout() {
        let facts = extract_facts(&normalize_text(SAMPLE));

        // 被硬换行切成两半的排名必须能拼回来。
        assert_eq!(facts.rank.as_deref(), Some("20"));
        assert_eq!(facts.major_size.as_deref(), Some("172"));
        assert_eq!(facts.weighted_average.as_deref(), Some("87.91"));
        assert_eq!(facts.gpa.as_deref(), Some("3.69"));
        assert_eq!(facts.deadline.as_deref(), Some("2026年08月14日"));

        // 页眉页脚与附录都不该混进正文。
        assert!(!facts.statement.contains("ACADEMIC AFFAIRS"));
        assert!(!facts.statement.contains("第1页"));
        assert!(!facts.statement.contains("等级换算关系表"));
        assert!(facts.statement.contains("厦门大学本科学分绩点排名证明"));
        assert!(facts.statement.ends_with("特此证明。"));
    }

    #[test]
    fn rank_marker_in_title_is_skipped() {
        // 标题“……学分绩点排名证明”里也有“绩点排名”，但后面不是数字，必须跳过。
        assert_eq!(extract_facts("厦门大学本科学分绩点排名证明").rank, None);
    }

    /// 有真实证明样本时才跑；样本由下面的联网测试自动落盘。
    #[tokio::test]
    async fn extract_sample_pdf() -> Result<()> {
        const SAMPLE_PATH: &str = "data/temp/gpa_certificate.pdf";
        let Ok(bytes) = tokio::fs::read(SAMPLE_PATH).await else {
            println!("[skip] 没有 {SAMPLE_PATH}，跳过真实样本提取测试");
            return Ok(());
        };
        let pdf = Bytes::from(bytes);
        let text = GpaCertificate::extract_text(pdf).await?;
        println!("提取正文:\n{text}");
        println!("关键字段: {:#?}", extract_facts(&text));
        assert!(!text.is_empty());
        Ok(())
    }

    /// 端到端跑一遍：列范围 → 申请/取结果 → 下载证明 → 提取正文。
    #[tokio::test]
    async fn download_and_extract() -> Result<()> {
        use crate::api::xmu_service::jw::{
            GpaApply, GpaRange, GpaRecord, GpaRecordResponse, get_castgc_client,
        };

        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let client = get_castgc_client(castgc);

        let ranges = GpaRange::get_from_client(&client).await?;
        println!("成绩范围: {ranges:#?}");
        let range = ranges.first().expect("至少应有一个成绩范围");

        // 与 web 层 query_handler 同样的「先查后申」：已有有效结果就不要再提交申请。
        // 教务对已有有效结果的重复申请不一定回 JSON 错误，实测会返回“系统异常”HTML 页。
        let mut rows = GpaRecord::get_from_client(&client).await?;
        if GpaRecord::find_valid(&rows, &range.wid).is_none() {
            let outcome = GpaApply::submit_from_client(&client, &range.wid, &range.xsmc).await?;
            println!("申请结果: {}", outcome.as_str());
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            rows = GpaRecord::get_from_client(&client).await?;
        } else {
            println!("申请结果: 该成绩范围已有有效结果，跳过申请");
        }

        println!("申请记录: {rows:#?}");
        let wid = GpaRecord::find_valid(&rows, &range.wid)
            .and_then(GpaRecordResponse::print_wid)
            .expect("应能找到该成绩范围的有效计算结果")
            .to_owned();

        let pdf = GpaCertificate::download_from_client(&client, &wid, true).await?;
        // 顺手落盘，方便 extract_sample_pdf 复跑与人工核对。
        tokio::fs::create_dir_all("data/temp").await.ok();
        tokio::fs::write("data/temp/gpa_certificate.pdf", &pdf).await.ok();

        let text = GpaCertificate::extract_text(pdf).await?;
        println!("提取正文:");
        println!("{text}");
        println!("关键字段: {:#?}", extract_facts(&text));
        assert!(!text.is_empty());
        Ok(())
    }
}
