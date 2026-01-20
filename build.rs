use base64::{Engine as _, engine::general_purpose};
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // 1. 声明依赖监听，只要这些目录或文件变化，就重新构建
    println!("cargo:rerun-if-changed=qqdata/face");
    println!("cargo:rerun-if-changed=qqdata/theme");

    let out_dir = env::var("OUT_DIR").unwrap();

    // --- 逻辑 A: 生成表情包 PHF Map ---
    generate_face_data(&out_dir);

    // --- 逻辑 B: 生成内嵌字体的 HTML 模板 ---
    generate_theme_template(&out_dir);
}

fn generate_face_data(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("face_data.rs");
    let mut builder = phf_codegen::Map::new();
    let face_dir = Path::new("qqdata/face");

    if let Ok(entries) = fs::read_dir(face_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gif") {
                let face_id = path.file_stem().unwrap().to_str().unwrap().to_string();
                let bytes = fs::read(&path).expect("无法读取表情文件");
                let b64_content = general_purpose::STANDARD.encode(bytes);

                let val_expr = format!(
                    "(r#\"image/gif\"#, r#\"{}\"# , r#\"{}.gif\"#)",
                    b64_content, face_id
                );
                builder.entry(face_id, val_expr);
            }
        }
    }

    let code = format!(
        "static FACES: phf::Map<&'static str, (&'static str, &'static str, &'static str)> = {};\n",
        builder.build()
    );
    fs::write(dest_path, code).unwrap();
}

fn generate_theme_template(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("theme_template.rs");
    let css_path = Path::new("qqdata/theme/theme.css");
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
