use base64::{Engine as _, engine::general_purpose};
use core::panic;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write; // 新增
use std::path::Path;
use syn::{Attribute, Lit, Meta, Token, punctuated::Punctuated}; // 新增

fn main() {
    // 1. 声明依赖监听
    println!("cargo:rerun-if-changed=app_data/face");
    println!("cargo:rerun-if-changed=app_data/theme");
    println!("cargo:rerun-if-changed=src/logic");
    println!("cargo:rerun-if-changed=src/api/xmu_service/lnt");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = env::var("OUT_DIR").unwrap();

    // --- 逻辑 A: 生成表情包 PHF Map ---
    generate_face_data(&out_dir);

    // --- 逻辑 B: 生成内嵌字体的 HTML 模板 ---
    generate_theme_template(&out_dir);

    // --- 逻辑 C: 生成 logic handlers 注册文件，直接写入 src/logic/mod.rs ---
    generate_logic_handlers();

    // --- 逻辑 D: 生成 lnt api 模块 ---
    generate_lnt_api_mod();

    // --- 逻辑 E: 生成 omikuji 签文数据模块 ---
    generate_omikuji_data(&out_dir).expect("Failed to generate omikuji data");

    // --- 逻辑 F: 生成二维码模型和图片数据 ---
    generate_qrcode_data(&out_dir);
}

fn generate_face_data(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("face_data.rs");
    let mut builder = phf_codegen::Map::new();
    let face_dir = Path::new("app_data/face");

    if let Ok(entries) = fs::read_dir(face_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gif") {
                let face_id = path.file_stem().unwrap().to_str().unwrap().to_string();
                //读取face_id.txt作为name
                let txt_path = path.with_extension("txt");
                let name = if txt_path.exists() {
                    fs::read_to_string(&txt_path)
                        .unwrap_or_else(|_| face_id.clone()) // 读取失败则回退
                        .trim() // 去掉前后换行和空格
                        .replace('"', "\\\"") // 防止内容里有引号导致代码报错
                        .to_string()
                } else {
                    panic!("表情 {} 缺失对应的描述文件 {}", face_id, txt_path.display());
                };
                let bytes = fs::read(&path).expect("无法读取表情文件");
                let b64_content = general_purpose::STANDARD.encode(bytes);

                let val_expr = format!(
                    "(r#\"image/gif\"#, r#\"{}\"# , r#\"{}\"#, r#\"{}\"#)",
                    b64_content, face_id, name
                );
                builder.entry(face_id, val_expr);
            }
        }
    }

    let code = format!(
        "static FACES: phf::Map<&'static str, (&'static str, &'static str, &'static str, &'static str)> = {};\n",
        builder.build()
    );
    fs::write(dest_path, code).unwrap();
}

fn generate_theme_template(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("theme_template.rs");
    let css_path = Path::new("app_data/theme/theme.css");
    let theme_dir = css_path.parent().expect("无法获取主题目录");

    // 读取基础 CSS
    let css_content = fs::read_to_string(css_path).unwrap_or_else(|_| "".to_string());

    // 自动扫描并内嵌所有 TTF 字体
    let mut font_faces = String::new();
    if let Ok(entries) = fs::read_dir(theme_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ttf") {
                let font_name = path.file_stem().unwrap().to_str().unwrap();
                let font_data = fs::read(&path).expect("读取字体失败");
                let b64_font = general_purpose::STANDARD.encode(font_data);

                font_faces.push_str(&format!(
                    r#"@font-face {{ font-family: '{}'; src: url(data:font/ttf;base64,{}) format('truetype'); }}"#,
                    font_name, b64_font
                ));
            }
        }
    }

    // 构造最终 HTML 框架
    let final_css = format!("{}\n{}", font_faces, css_content);
    let html_template = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>{}</style></head><body class="typora-export"><div id="write" class="markdown-body">{{{{content}}}}</div></body></html>"#,
        final_css
    );

    let code = format!("pub const HTML_TEMPLATE: &str = {:?};", html_template);
    fs::write(dest_path, code).unwrap();
}

// 将 snake_case 转换为 PascalCase 并加上 Handler 后缀
fn to_pascal_case_handler(s: &str) -> String {
    let pascal = s
        .split('_')
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<String>();
    format!("{}Handler", pascal)
}

// 检查函数属性中是否存在 #[handler(..., help_msg="...")]
fn is_handler(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if attr.path().is_ident("handler") {
            return true;
        }
    }
    false
}

fn is_command_handler(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if attr.path().is_ident("handler") {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;

            if let Ok(nested) = attr.parse_args_with(parser) {
                for meta in nested {
                    if let Meta::NameValue(nv) = meta
                        && nv.path.is_ident("help_msg")
                    {
                        // 找到了 help_msg
                        if let syn::Expr::Lit(expr_lit) = &nv.value
                            && let Lit::Str(_) = &expr_lit.lit
                        {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// 解析单个文件中的 handlers 并将它们分类
fn parse_file_for_handlers(
    file_path: &Path,
    module_prefix: &str,
    command_handlers: &mut Vec<String>,
    other_handlers: &mut Vec<String>,
) {
    // 使用 expect! 替代 ? 确保在 build.rs 中失败时 panic
    let content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| panic!("Failed to read file: {}", file_path.display()));
    let syntax = syn::parse_file(&content)
        .unwrap_or_else(|_| panic!("Failed to parse file: {}", file_path.display()));

    for item in syntax.items {
        if let syn::Item::Fn(func) = item {
            // 只有标记了 #[handler] 的函数才会被视为 Handler
            if !is_handler(&func.attrs) {
                continue;
            }

            let fn_name = func.sig.ident.to_string();

            let is_command = is_command_handler(&func.attrs);

            // 构造 Handler 结构体名
            let handler_struct_name = to_pascal_case_handler(&fn_name);

            // 完整的 Handler 引用路径：module_prefix::HandlerStructName
            let handler_path = format!("{}::{}", module_prefix, handler_struct_name);

            if is_command {
                command_handlers.push(handler_path);
            } else {
                other_handlers.push(handler_path);
            }
        }
    }
}

fn generate_logic_handlers() {
    let root = Path::new("src/logic");
    let mut command_handlers = Vec::new();
    let mut other_handlers = Vec::new();
    let mut all_mod_declarations = Vec::new();

    // 1. 扫描 src/logic 目录
    for entry in fs::read_dir(root).expect("Failed to read src/logic directory") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();
        let file_name = entry.file_name().into_string().unwrap_or_default();

        // 声明依赖监听
        println!("cargo:rerun-if-changed={}", path.display());

        if path.is_file() && file_name.ends_with(".rs") && file_name != "mod.rs" {
            let mod_name = file_name.strip_suffix(".rs").unwrap();

            // 模块声明 (mod echo;)
            all_mod_declarations.push(format!("pub mod {};", mod_name));

            // 扫描文件中的 handlers
            parse_file_for_handlers(&path, mod_name, &mut command_handlers, &mut other_handlers);
        } else if path.is_dir() {
            // 2. 处理子目录模块 (例如 login, helper)
            let dir_name = file_name;

            // 模块目录声明 (mod login;)
            all_mod_declarations.push(format!("pub mod {};", dir_name));

            // 扫描子目录下的所有 .rs 文件 (排除 mod.rs)
            for sub_entry in fs::read_dir(&path)
                .unwrap_or_else(|_| panic!("Failed to read directory: {}", path.display()))
            {
                let sub_entry = sub_entry.expect("Failed to read sub-directory entry");
                let sub_path = sub_entry.path();
                let sub_file_name = sub_entry.file_name().into_string().unwrap_or_default();

                if sub_path.is_file() && sub_file_name.ends_with(".rs") && sub_file_name != "mod.rs"
                {
                    // 声明依赖监听
                    println!("cargo:rerun-if-changed={}", sub_path.display());

                    // 扫描子模块文件中的 handlers，使用 dir_name 作为前缀 (例如 login::LoginHandler)
                    parse_file_for_handlers(
                        &sub_path,
                        &dir_name,
                        &mut command_handlers,
                        &mut other_handlers,
                    );
                }
            }
        }
    }

    // 3. 收集 Handler 路径字符串 (保持为 String)
    let cmd_args = command_handlers.to_vec().join(",\n        ");

    let other_args = other_handlers.to_vec().join(",\n        ");

    // 4. 生成代码字符串 (硬编码格式)
    let mut generated_string = String::new();

    // 自动生成的模块声明 (确保每个声明独占一行)
    for decl in all_mod_declarations.iter() {
        generated_string.push_str(decl);
        generated_string.push('\n');
    }
    generated_string.push('\n'); // 额外空行

    let code_body = format!(
        r#"use crate::abi::logic_import::*;

pub trait BuildHelp {{
    const HELP_MSG: &'static str;
}}

register_handler_with_help!(
    command = [
        {}
    ],
    other = [
        {}
    ]
);
"#,
        cmd_args, other_args
    );

    generated_string.push_str(&code_body);

    // 5. 写入 src/logic/mod.rs
    let dest_path = Path::new("src/logic/mod.rs");

    let top_comment = r#"// NOTE: This file is automatically generated by build.rs. Do not edit manually.
// 自动扫描 logic 目录下的 handlers。
// 规则: 带有 help_msg 的 handler 放入 command 列表, 否则放入 other 列表。
// 假设: 所有 Handler 结构体都通过其模块的根路径暴露 (例如 `echo::EchoHandler`, `login::LoginHandler`)。

"#;
    let full_content = format!("{}{}", top_comment, generated_string);

    fs::write(dest_path, full_content.as_bytes()).expect("Failed to write to src/logic/mod.rs");
}

fn parse_lnt_file_for_structs(file_path: &Path, structs: &mut Vec<String>) {
    let content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| panic!("Failed to read file: {}", file_path.display()));
    let syntax = syn::parse_file(&content)
        .unwrap_or_else(|_| panic!("Failed to parse file: {}", file_path.display()));

    for item in syntax.items {
        if let syn::Item::Struct(st) = item {
            // 只有 pub 结构体才考虑
            if !matches!(st.vis, syn::Visibility::Public(_)) {
                continue;
            }

            let mut has_lnt_get_api = false;
            for attr in &st.attrs {
                if attr.path().is_ident("lnt_get_api") {
                    has_lnt_get_api = true;
                    break;
                }
            }

            if has_lnt_get_api {
                structs.push(st.ident.to_string());
            } else if matches!(st.fields, syn::Fields::Unit) {
                // 如果是 pub 且是 Unit Struct (没有内容的结构体)，也导出
                structs.push(st.ident.to_string());
            }
        }
    }
}

fn generate_lnt_api_mod() {
    let root = Path::new("src/api/xmu_service/lnt");
    let mut all_mod_declarations = Vec::new();
    let mut all_reexports = Vec::new();

    println!("cargo:rerun-if-changed=src/api/xmu_service/lnt");

    let mut entries: Vec<_> = fs::read_dir(root)
        .expect("Failed to read src/api/xmu_service/lnt directory")
        .map(|r| r.expect("Failed to read entry"))
        .collect();

    // 排序以保证生成的文件内容顺序稳定
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name().into_string().unwrap_or_default();

        if path.is_file() && file_name.ends_with(".rs") && file_name != "mod.rs" {
            let mod_name = file_name.strip_suffix(".rs").unwrap();
            all_mod_declarations.push(format!("pub mod {};", mod_name));

            let mut structs = Vec::new();
            parse_lnt_file_for_structs(&path, &mut structs);
            for s in structs {
                all_reexports.push(format!("pub use {}::{};", mod_name, s));
            }
        }
    }

    let mut generated_string = String::new();
    generated_string.push_str(
        "// NOTE: This file is automatically generated by build.rs. Do not edit manually.\n\
        // 自动扫描 lnt 目录下的 .rs 文件。\n\
        // 规则: \n\
        // 1. 带有 #[lnt_get_api(...)] 宏标记的 pub 结构体会被自动 re-export。\n\
        // 2. 所有 pub 且为 Unit Struct (无内容结构体，如 `pub struct XXX;`) 的结构体也会被自动 re-export。\n\n",
    );

    for decl in all_mod_declarations {
        generated_string.push_str(&decl);
        generated_string.push('\n');
    }
    generated_string.push('\n');

    for reexport in all_reexports {
        generated_string.push_str(&reexport);
        generated_string.push('\n');
    }

    generated_string.push_str(
        r#"
use crate::api::network::SessionClient;
use std::sync::LazyLock;
use url::Url;
use url_macro::url;

pub static LNT_URL: LazyLock<Url> = LazyLock::new(|| url!("https://lnt.xmu.edu.cn"));

pub fn get_session_client(session: &str) -> SessionClient {
    let client = SessionClient::new();
    client.set_cookie("session", session, &LNT_URL);
    client
}
"#,
    );

    let dest_path = root.join("mod.rs");
    fs::write(dest_path, generated_string.as_bytes())
        .expect("Failed to write to src/api/xmu_service/lnt/mod.rs");
}

// --- 逻辑 E: 生成 omikuji 签文数据模块 ---

// Struct to hold the actual parsed fortune data before writing to code
#[derive(Debug, Deserialize)]
struct SensoJiStickData {
    qcs: Vec<Vec<String>>,
}

/// Helper function to escape strings for embedding in Rust string literals.
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\"', "\\\"")
        .replace('\r', "")
}

fn generate_omikuji_data(out_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dest_path = Path::new(&out_dir).join("omikuji.rs");
    let mut file = fs::File::create(&dest_path)?;

    let omikuji_dir = Path::new("app_data/omikuji");
    let senso_ji_path = omikuji_dir.join("senso-ji-stick-data.json");
    let ruanyf_path = omikuji_dir.join("ruanyf-fortune.txt");

    // 1. 设置依赖 (rerun-if-changed)
    // 依赖于整个 app_data/omikuji/ 目录，当目录内容变化时，build.rs 会重新运行
    println!("cargo:rerun-if-changed=app_data/omikuji/");

    // 2. 扫描目录并检查文件使用情况 (如果存在未使用的文件则 Panic)
    let used_files = vec![
        senso_ji_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
        ruanyf_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
    ];
    let found_files: Vec<String> = fs::read_dir(omikuji_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            // 排除子目录
            if path.is_file() {
                Some(path.file_name().unwrap().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    // 检查是否有额外的文件或缺失的文件
    let mut unused_files_found = false;
    if found_files.len() != used_files.len() {
        unused_files_found = true;
    } else {
        for found_file in found_files.iter() {
            if !used_files.contains(found_file) {
                unused_files_found = true;
                break;
            }
        }
    }

    if unused_files_found {
        panic!(
            "Error: app_data/omikuji/ directory contains unused files. Expected: {:?}, Found: {:?}",
            used_files, found_files
        );
    }

    // 3. 解析 senso-ji-stick-data.json
    let senso_ji_content = fs::read_to_string(&senso_ji_path)?;
    let raw_data: SensoJiStickData =
        serde_json::from_str(&senso_ji_content).expect("Failed to parse senso-ji-stick-data.json");

    // 过滤掉标题行，并生成格式化后的签文字符串
    let senso_ji_fortunes_str: Vec<String> = raw_data
        .qcs
        .into_iter()
        .skip(1) // 跳过标题: [ "浅草寺观音签" ]
        .filter_map(|mut parts| {
            // 确保有足够的元素 (type, verse, meaning, wish, caution)
            if parts.len() >= 5 {
                let type_str = parts.remove(0).trim().to_string();
                let verse = parts.remove(0).trim().replace('\n', " ").to_string();
                let meaning = parts.remove(0).trim().replace('\n', " ").to_string();
                let wish = parts.remove(0).trim().replace('\n', " ").to_string();
                let caution = parts.remove(0).trim().replace('\n', " ").to_string();

                // 编译期拼接
                Some(format!(
                    "浅草寺观音签 ({}):\n\n签诗: {}\n\n解释: {}\n\n愿望: {}\n\n忠告: {}",
                    type_str, verse, meaning, wish, caution
                ))
            } else {
                eprintln!("Skipping malformed Senso-ji fortune part: {:?}", parts);
                None
            }
        })
        .collect();

    // 4. 解析 ruanyf-fortune.txt
    let ruanyf_content = fs::read_to_string(&ruanyf_path)?;
    let ruanyf_fortunes: Vec<String> = ruanyf_content
        .split('%')
        .filter_map(|s| {
            let cleaned = s.trim();
            if cleaned.is_empty() {
                None
            } else {
                // 移除 UTF-8 BOM (U+FEFF)，并清理字符串
                Some(cleaned.replace('\u{feff}', "").trim().to_string())
            }
        })
        .collect();

    // 5. 生成 Rust 代码到 omikuji.rs

    // 5.1. 模块注释 (SensoJiFortune 结构体现在仅用于 build.rs 内部解析)
    writeln!(
        file,
        "// omikuji 模块由 build.rs 自动生成，包含签文数据和随机抽取 API。"
    )?;
    writeln!(file)?;

    // 5.2. Senso-ji 数组
    writeln!(file, "pub const SENSO_JI_FORTUNES: &[&str] = &[")?;
    for fortune_str in senso_ji_fortunes_str.iter() {
        writeln!(file, "    \"{}\",", escape_string(fortune_str))?;
    }
    writeln!(file, "];")?;
    writeln!(file, "\n")?;

    // 5.3. Ruanyf 数组
    writeln!(file, "pub const RUANYF_FORTUNES: &[&str] = &[")?;
    for fortune in ruanyf_fortunes.iter() {
        writeln!(file, "    \"{}\",", escape_string(fortune))?;
    }
    writeln!(file, "];")?;
    writeln!(file, "\n")?;

    // 5.4. API
    // 注意：rand 已经在 main crate dependencies 中，但在 build.rs 生成的文件中需要声明 use
    writeln!(file, "use rand::Rng;")?;

    // Senso-ji API
    writeln!(file, "/// 随机获取一条浅草寺签文。")?;
    writeln!(
        file,
        "pub fn random_senso_ji_fortune(rng: &mut rand::rngs::SmallRng) -> &'static str {{"
    )?;
    writeln!(
        file,
        "    let index = rng.random_range(0..SENSO_JI_FORTUNES.len());"
    )?;
    writeln!(file, "    SENSO_JI_FORTUNES[index]")?;
    writeln!(file, "}}")?;
    writeln!(file, "\n")?;

    // Ruanyf API
    writeln!(file, "/// 随机获取一条阮一峰语录。")?;
    writeln!(
        file,
        "pub fn random_ruanyf_fortune(rng: &mut rand::rngs::SmallRng) -> &'static str {{"
    )?;
    writeln!(
        file,
        "    let index = rng.random_range(0..RUANYF_FORTUNES.len());"
    )?;
    writeln!(file, "    RUANYF_FORTUNES[index]")?;
    writeln!(file, "}}")?;

    // 确保生成文件写入完成
    file.flush()?;

    Ok(())
}

fn generate_qrcode_data(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("qrcode_data.rs");
    let model_dir = Path::new("app_data/wechat_qrcode");
    let preload_jpg = Path::new("app_data/preload_qrcode.jpg");

    println!("cargo:rerun-if-changed=app_data/wechat_qrcode");
    println!("cargo:rerun-if-changed=app_data/preload_qrcode.jpg");

    let mut code = String::new();
    code.push_str(
        "// NOTE: This file is automatically generated by build.rs. Do not edit manually.\n\n",
    );

    let files = [
        ("DETECT_PROTOTXT", "detect.prototxt"),
        ("DETECT_CAFFEMODEL", "detect.caffemodel"),
        ("SR_PROTOTXT", "sr.prototxt"),
        ("SR_CAFFEMODEL", "sr.caffemodel"),
    ];

    for (const_name, file_name) in files {
        let path = model_dir.join(file_name);
        code.push_str(&format!(
            "pub const {}: &[u8] = include_bytes!(r#\"{}\"#);\n",
            const_name,
            fs::canonicalize(&path)
                .unwrap()
                .display()
                .to_string()
                .replace(r"\\?\", "")
        ));
    }

    code.push_str(&format!(
        "pub const PRELOAD_QRCODE_JPG: &[u8] = include_bytes!(r#\"{}\"#);\n",
        fs::canonicalize(preload_jpg)
            .unwrap()
            .display()
            .to_string()
            .replace(r"\\?\", "")
    ));

    fs::write(dest_path, code).unwrap();
}
