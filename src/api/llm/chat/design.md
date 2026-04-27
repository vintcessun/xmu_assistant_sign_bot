## Plan: Chat 架构对标重构最终版

目标是在不依赖本地模型的前提下，彻底移除转写链路，提升群聊高频场景下的响应质量与吞吐，修复文件可用性与副本问题，并把上下文和记忆从“短窗口”升级为“分层记忆”。方案借鉴 AstrBot 公开文档中的上下文压缩与多媒体直发思路，以及 OpenAI 公开文档中的提示词缓存与异步批处理思路。

**对标结论（外部方案 vs 当前实现）**
1. 当前 L0 是 MessageAbstract{qq,msg_text} 精确命中，命中率低；成熟方案使用归一化键与群级热点键。
2. 当前存在生成后转写的隐藏 LLM 调用，导致格式泄露与高延迟；成熟方案使用一步结构化输出。
3. 当前文件去重主要基于 URL 映射，导致同内容不同 URL 形成副本；成熟方案使用内容哈希主索引。
4. 当前高频群聊每条消息都尝试回复，容易刷屏；成熟方案采用回复预算与节流窗口。
5. 当前记忆主要依赖短时间窗口，长期关联弱；成熟方案采用“短窗 + 摘要记忆 + 检索回填”。
6. 当前缺少稳定的人设层与命令代理层；成熟方案强调“角色策略与工具调用策略解耦”，避免人格漂移与越权执行。
7. 自动生成命令目录当前依赖 help_msg 识别 command handler，若新增命令缺少 help_msg 将不会进入 command 列表，Broker 需兼容该约束并给出启动期告警。

**Steps**
1. Phase A: 移除转写模块，改为一步到位结构化生成（阻塞后续）
2. 文件: src/api/llm/chat/message/bridge.rs, src/api/llm/chat/deep/send.rs, src/api/llm/chat/search/send.rs, src/api/llm/chat/llm.rs
3. 删除 IntoMessageSend::get 中对 ask_as_high<MessageSendLlmResponse> 的转写调用，保留纯解析职责。
4. 将生成入口改为直接输出 MessageSendLlmResponse 或 LlmDeepReply（含 segments），实现一次调用完成“是否回复+回复内容”。
5. 将原转写 system prompt 中的格式说明迁移到生成 prompt 的开发者层，并与用户历史上下文隔离，防止格式污染。
6. 输出失败时只进行本地解析重试，不再发起额外 LLM 转写回路。

7. Phase B: 重做 L0 命中策略（并行于 Phase C）
8. 文件: src/api/llm/chat/repeat/reply.rs, src/api/llm/chat/repeat/send.rs, src/api/llm/chat/router.rs
9. 将 L0 键从 qq+原文 改为 group_id+normalized_text，normalized_text 包含空白折叠、标点归一、全半角归一、大小写归一、emoji 占位归一。
10. 新增“双层 L0”：L0a 为完全归一化精确命中；L0b 采用 trigram + Jaccard（最终选型），不使用 Levenshtein（高频场景 O(mn) 成本高）也不在 L0 引入 embedding（避免额外延迟与费用）。
11. 增加 L0 命中统计与自动降级：若 24h 命中率低于阈值，则跳过 L0b 仅保留 L0a，避免无效计算。
12. 在 router 增加短路策略：高置信 L0 命中直接返回，避免进入 L1/L2。

13. Phase C: 高频防刷屏与回复预算（并行于 Phase B）
14. 文件: src/api/llm/chat/router.rs, src/api/llm/chat/deep/send.rs, src/api/llm/chat/search/send.rs
15. 增加 group 级令牌桶：默认每群每 60 秒最多 10 条主动回复（可配置），AT/私聊不受限。
16. 增加 debounce 聚合窗口：5-8 秒内多条非 AT 消息合并为一次决策输入。
17. 增加“只看两眼”快速门控：短消息、寒暄、水消息、纯表情优先不回复。
18. 增加“最后发言保护”：同一用户连续触发时，优先延迟或跳过，减少对话被机器人占用。

19. Phase D: 分层上下文与记忆增强（依赖 Phase A）
20. 文件: src/api/llm/chat/archive/message_storage.rs, src/api/llm/chat/archive/memo_fragment.rs, src/api/llm/chat/deep/send.rs, src/api/llm/chat/search/send.rs
21. 将 L2 上下文从 120 秒窗口改为每群最近 30 条消息。
22. 当上下文 token 估算达到模型窗口 80%-82% 时触发压缩（借鉴 AstrBot 阈值策略）：先摘要压缩，再必要时按轮截断。
23. 将压缩摘要写入 MemoFragment，并在后续 L2 通过向量检索回填 TopK 摘要。
24. 新增“事实记忆”和“会话记忆”分离：事实记忆长期保留，闲聊记忆短 TTL。

25. Phase E: 文件可靠性与去副本（依赖 Phase A，部分可并行）
26. 文件: src/api/llm/chat/file/mod.rs, src/api/llm/chat/archive/bridge.rs, src/api/llm/chat/archive/file_embedding.rs
27. 去重主键改为内容哈希（全 SHA-256），URL 仅作别名索引，不再作为去重依据。
28. from_url 下载后先算哈希查重：已存在则复用 canonical 文件路径并只追加 URL 别名映射。
29. 归档失败分支禁止 Binary::from_url 持久化，改为本地占位并后台重试下载，成功后回填本地 Binary::from_file。
30. embedding 前增加“按 file_id 幂等检查”，避免重复插入向量库。
31. 增加副本清理任务：扫描同 hash 多路径文件，保留 canonical，其余删除并修复索引。

32. Phase F: 成本与延迟优化（依赖 Phase A-D）
33. 文件: src/api/llm/chat/llm.rs, src/api/llm/chat/config.rs, src/api/llm/chat/audit/main.rs
34. 模型分级：L2 非 AT 使用 Flash，AT 或高风险请求使用 Pro；L0/L1 审计采样并固定低成本模型。
35. 提示词前缀稳定化：固定系统前缀放最前，动态上下文放末尾，提升 API 端 prompt cache 命中率。
36. 将异步任务移入离线队列：审计反思、批量 embedding、低优先回顾可走异步批处理，减少在线延迟与费用。

37. Phase G: 人设层与命令代理层（依赖 Phase A，关键新增）
38. 文件: src/api/llm/chat/deep/send.rs, src/api/llm/chat/search/send.rs, src/api/llm/chat/config.rs, src/logic/mod.rs, src/abi/router/context.rs
39. 固化人设：新增 AgentPersona 配置，默认“李老师”，要求诙谐幽默但不油腻，优先用简短自然句回应反问与追问。
40. 人设与业务解耦：人设 prompt 只控制语气与互动，不参与权限决策与工具路由。
41. 新增 LogicCommandBroker（命令代理）：暴露 logic 下全部 command handler 为可调用命令目录，目录由 build.rs 自动生成的 src/logic/mod.rs command 列表驱动，随命令扩展自动更新工具库，无需人工维护。
42. 不重建 Context：命令代理直接复用当前会话 Context（保持 message_list、状态与链路一致），仅在执行前向 message_text 前插固定指令前缀。
43. 命令执行统一走 Broker 调度，保证调用路径一致、日志一致、上下文写回一致。
44. 命令零限制策略：不做审批、不做校验、不做限流，所有 logic command 默认可执行，按正常逻辑直接落到当前会话链路。
45. Phase H: 一致性执行链路补强（依赖 Phase G）
46. 文件: build.rs, src/logic/mod.rs, helper/src/lib.rs, src/api/llm/chat/router.rs, src/api/llm/chat/config.rs
47. 启动期一致性检查：对比 build.rs 生成的 command 列表与 Broker 工具目录，发现缺失即告警（仅观测，不阻断执行）。
48. 命令匹配歧义治理：沿用 build.rs 的“长命令优先”排序并输出冲突日志，减少前缀命令误触发。
49. 固定前缀注入：在 Broker 调用前统一给 message_text 注入固定指令前缀，再按原 handler 分发执行，执行结果写回当前上下文与当前会话文件链路。
50. 上下文类型安全补强：保留现有分发能力，优先保证“上下文一致性执行”在所有命令路径成立。

51. Phase I: 观测与灰度（最终收口）
52. 文件: src/api/llm/chat/router.rs, src/logger.rs, src/config.rs
53. 新增核心指标：L0 命中率、每群回复率、平均响应延迟、每条消息 token 成本、文件去重率、记忆命中率、命令代理成功率、命令拒绝率、命令冲突率。
54. 采用配置开关灰度上线：先开 Phase A+C，再开 G（人设+代理），再开 H（严密性补强），再开 D+E，最后开 B+F。
55. 设置回滚条件：刷屏率上升、延迟恶化或命令误调用率超过阈值时自动回退到上一策略。

**Relevant files**
- src/api/llm/chat/message/bridge.rs — 删除转写 LLM 依赖，保留结构化解析与段落映射
- src/api/llm/chat/deep/send.rs — 单次调用决策+生成、回复门控、上下文注入
- src/api/llm/chat/search/send.rs — L1 回复节流与结构化输出对齐
- src/api/llm/chat/repeat/reply.rs — L0 键归一化与近似命中
- src/api/llm/chat/router.rs — 预算、节流、聚合、短路
- src/api/llm/chat/archive/message_storage.rs — 每群窗口与压缩触发
- src/api/llm/chat/archive/memo_fragment.rs — 摘要记忆回填
- src/api/llm/chat/file/mod.rs — 内容哈希去重、URL 别名索引
- src/api/llm/chat/archive/bridge.rs — 禁止在线 URL 持久化与失败回填
- src/api/llm/chat/archive/file_embedding.rs — embedding 幂等与去重
- src/api/llm/chat/audit/main.rs — 审计异步与采样策略
- src/api/llm/chat/config.rs — 模型分级、人设配置与灰度开关
- src/logic/mod.rs — command 列表来源与命令目录导出
- src/abi/router/context.rs — 纯净上下文克隆接口

**Verification**
1. 功能验证：转写模块关闭后，L2 仍能生成 Text/Image/Face/At/File 并正常发送。
2. 性能验证：对比改造前后 P50/P95 响应时间，目标 L2 延迟下降 35% 以上。
3. 反刷屏验证：高频群聊回放中，机器人发言占比低于 15%，AT 回复率保持 99% 以上。
4. 记忆验证：跨 24 小时对话回测中，事实类问题召回率显著高于旧方案。
5. 文件验证：同内容不同 URL 上传后仅保留一份 canonical 文件，在线链接失效不影响历史可读。
6. 成本验证：单日 token 成本较基线下降（目标 25%-40%）。
7. 人设验证：同一场景下语气稳定为“李老师”风格，且不影响事实准确率与命令成功率。
8. 代理验证：LLM 仅能通过 LogicCommandBroker 调用 logic 命令，无法绕过到未注册内部函数。
9. 上下文安全验证：clone_clean_for_command 不携带上次工具副作用状态，跨命令无脏数据泄漏。

**Decisions**
- 仅参考公开资料与公开文档，不引入任何泄露代码。
- 彻底取消“生成后再转写”的架构，结构化输出一步到位。
- L0 从“用户级精确匹配”改为“群级归一化命中 + 近似命中”。
- 对高频群聊采用预算优先而非每条消息必回。
- 人设层与命令执行层分离：李老师风格可变更，但命令执行链路与上下文写回规则不由人设覆盖。

**Further Considerations**
1. L0b 已选 trigram/Jaccard，待确定阈值与分词参数（例如 trigram 窗口、Jaccard 阈值）以平衡召回与误触发。
2. 记忆压缩模型是否独立：可用低成本模型做摘要，主模型仅负责生成。
3. 文件 canonical 存储目录是否单独分区，避免与临时下载目录混用。
4. 固定指令前缀文案与插入位置（message_text 头部）需要定稿，以保证所有命令执行行为一致。