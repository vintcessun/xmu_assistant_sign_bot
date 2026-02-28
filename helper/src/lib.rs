extern crate proc_macro;
use core::panic;

use darling::FromMeta;
use heck::AsPascalCase;
use proc_macro::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Attribute, Error, Expr, FnArg, Ident, ItemFn, ItemStruct, Lit, LitBool, LitStr, Meta, Pat,
    Path, Result, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
    token,
};

struct ApiAttr {
    path: LitStr,
    _comma: token::Comma,
    response_type: Type,
}

impl Parse for ApiAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(ApiAttr {
            path: input.parse()?,
            _comma: input.parse()?,
            response_type: input.parse()?,
        })
    }
}

#[proc_macro_attribute]
pub fn api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ApiAttr);
    let path = args.path.value();
    let response_type = args.response_type;

    if !path.starts_with('/') || path.len() < 2 {
        return Error::new(
            args.path.span(),
            "Must start with '/' and have an action name (e.g., #[api(\"/send_msg\", MyResponse)])",
        )
        .to_compile_error()
        .into();
    }

    let action_str = &path[1..];

    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let expanded = quote! {
        #[derive(Serialize, Deserialize, Debug)]
        #input

        impl Params for #name {
            type Response = #response_type;

            const ACTION: &'static str = #action_str;
        }
    };

    expanded.into()
}

#[proc_macro]
pub fn define_default_type(input: TokenStream) -> TokenStream {
    let input_str = input.to_string();
    let parts: Vec<&str> = input_str.split(',').map(|s| s.trim()).collect();

    if parts.len() != 3 {
        return quote! { compile_error!("Expected 3 arguments: name, type, default_val"); }.into();
    }

    let name = syn::parse_str::<syn::Ident>(parts[0]).unwrap();
    let ty = syn::parse_str::<syn::Type>(parts[1]).unwrap();
    let default_val = syn::parse_str::<syn::Expr>(parts[2]).unwrap();

    let expanded = quote! {
        #[derive(Debug, Clone, PartialEq, serde::Serialize)]
        pub struct #name(pub #ty);

        impl Default for #name {
            fn default() -> Self {
                Self(#default_val)
            }
        }

        impl<'de> serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: serde::Deserializer<'de> {
                let opt = Option::<#ty>::deserialize(deserializer)?;
                Ok(Self(opt.unwrap_or_else(|| #default_val)))
            }
        }

        impl std::ops::Deref for #name {
            type Target = #ty;
            fn deref(&self) -> &Self::Target { &self.0 }
        }
    };
    expanded.into()
}

#[derive(Debug, FromMeta)]
struct HandlerArgs {
    msg_type: Option<Ident>,
    command: Option<LitStr>,
    echo_cmd: bool,
    help_msg: Option<String>,
}

impl Parse for HandlerArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut msg_type = None;
        let mut command = None;
        let mut echo_cmd = false;
        let mut help_msg = None;

        let pairs = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for meta in pairs {
            let path = meta.path();
            if path.is_ident("msg_type") {
                if let Meta::NameValue(nv) = meta {
                    let expr = nv.value;
                    msg_type = Some(syn::parse2::<Ident>(quote!(#expr))?);
                }
            } else if path.is_ident("command") {
                if let Meta::NameValue(nv) = meta {
                    let expr = nv.value;
                    command = Some(syn::parse2::<LitStr>(quote!(#expr))?);
                }
            } else if path.is_ident("echo_cmd") {
                if let Meta::NameValue(nv) = meta {
                    let expr = nv.value;
                    // 解析 echo_cmd = true/false
                    let lit: LitBool = syn::parse2(quote!(#expr))?;
                    echo_cmd = lit.value;
                }
            } else if path.is_ident("help_msg") {
                if let Meta::NameValue(nv) = meta {
                    let expr = nv.value;
                    let val = syn::parse2::<LitStr>(quote!(#expr))?;
                    help_msg = Some(val.value());
                }
            } else {
                return Err(syn::Error::new_spanned(
                    path,
                    "Unknown attribute key, expected 'msg_type', 'command', 'echo_cmd', 'help_msg'",
                ));
            }
        }

        if echo_cmd && !msg_type.as_ref().map(|t| *t == "Message").unwrap_or(false) {
            return Err(syn::Error::new_spanned(
                &msg_type,
                "When 'echo_cmd' is true, 'msg_type' must be 'Message'",
            ));
        }

        if command.is_some() && help_msg.is_none() {
            return Err(syn::Error::new_spanned(
                &msg_type,
                "The 'help_msg' attribute is required because the help message is necessary for the handler.",
            ));
        }

        if let Some(ref cmd) = command
            && cmd.value().len() < 2
        {
            return Err(syn::Error::new_spanned(
                cmd,
                "The 'command' attribute must be at least 2 characters long.",
            ));
        }

        Ok(HandlerArgs {
            msg_type,
            command,
            echo_cmd,
            help_msg,
        })
    }
}

#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let args = parse_macro_input!(attr as HandlerArgs);

    let fn_name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let body = &input_fn.block;

    let target_type_ident = args.msg_type.clone().unwrap_or_else(|| format_ident!("M"));

    let struct_name = format_ident!(
        "{}Handler",
        AsPascalCase(fn_name.to_string()).to_string(),
        span = fn_name.span()
    );

    let hidden_impl = format_ident!("__hidden_{}_impl", fn_name);

    let type_const = if let Some(ref ty) = args.msg_type {
        quote! { Some(Type::#ty) }
    } else {
        quote! { None }
    };

    let cmd_const = if let Some(ref cmd) = args.command {
        let clean_cmd = cmd.value().replace('_', "");
        let clean_fn = fn_name.to_string().replace('_', "");
        if clean_cmd != clean_fn {
            return Error::new_spanned(
                cmd,
                "The 'command' attribute must match the function name with underscores removed.",
            )
            .to_compile_error()
            .into();
        }
        quote! { Some(#cmd) }
    } else {
        quote! { None }
    };

    let echo_logic = if args.echo_cmd {
        quote! {
            {
                let mut ctx = typed_ctx;
                ctx.set_echo();
                ctx
            }
        }
    } else {
        quote! {
            typed_ctx
        }
    };

    let (generics, target_type) = if args.msg_type.is_some() {
        (quote! { <T> }, quote! { #target_type_ident })
    } else {
        (
            quote! { <T, M: MessageType + std::fmt::Debug> },
            quote! { M },
        )
    };

    let help_trait = if args.command.is_some() {
        let cmd_val = args.command.as_ref().unwrap().value();
        let help_val = args.help_msg.as_ref().unwrap();
        let help_msg = format!("指令: {}\n{}\n\n", cmd_val, help_val);

        quote! {
            impl BuildHelp for #struct_name {
                const HELP_MSG: &'static str = #help_msg;
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[allow(non_upper_case_globals)]
        #vis const #fn_name: #struct_name = #struct_name;

        async fn #hidden_impl #generics(mut ctx: Context<T, #target_type>) -> anyhow::Result<()>
        where T: BotClient + BotHandler + std::fmt::Debug + 'static
        {
            let result: anyhow::Result<()> = (async { #body }).await;;
            if let Err(e) = result{
                handle_error(&mut ctx, stringify!(#fn_name), e).await;
            }
            ctx.finish().await;
            Ok(())
        }

        #[derive(Clone, Default, Debug)]
        #vis struct #struct_name;

        impl<T, M> Handler<T, M> for #struct_name
        where
            T: BotClient + BotHandler + std::fmt::Debug + 'static,
            M: MessageType + std::fmt::Debug + Send + Sync + 'static,
        {
            const FILTER_TYPE: Option<Type> = #type_const;
            const FILTER_CMD: Option<&'static str> = #cmd_const;

            #[inline(always)] //因为后面设计复杂的匹配逻辑并且强依赖死代码消除(DCE)所以这里强制内联
            fn handle(&self, ctx: &Context<T, M>) -> anyhow::Result<()> {
                let ctx = ctx.clone();
                let typed_ctx = unsafe {
                    std::mem::transmute::<Context<T, M>, Context<T, #target_type_ident>>(ctx)
                };
                let handle_ctx = #echo_logic;

                tokio::spawn(#hidden_impl(handle_ctx));

                Ok(())
            }
        }

        #help_trait
    };

    TokenStream::from(expanded)
}

struct RegisterInput {
    cmd_handlers: Vec<Path>,
    other_handlers: Vec<Path>,
}

impl Parse for RegisterInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut cmd_handlers = Vec::new();
        let mut other_handlers = Vec::new();

        while !input.is_empty() {
            let label: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let content;
            syn::bracketed!(content in input);
            let paths: Punctuated<Path, Token![,]> =
                content.parse_terminated(Path::parse, Token![,])?;

            if label == "command" {
                cmd_handlers.extend(paths);
            } else if label == "other" {
                other_handlers.extend(paths);
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(RegisterInput {
            cmd_handlers,
            other_handlers,
        })
    }
}

#[proc_macro]
pub fn register_handler_with_help(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RegisterInput);

    let cmd_handlers = &input.cmd_handlers;
    let other_handlers = &input.other_handlers;

    let mut all_cmds = cmd_handlers.clone();
    let help_handler_path: Path = syn::parse_str("HelpHandler").unwrap();
    all_cmds.push(help_handler_path);

    let expanded = quote! {
        #[handler(
            msg_type = Message,
            command = "help",
            echo_cmd = true,
            help_msg = "用法:/help\n功能:显示所有指令帮助"
        )]
        pub async fn help<T>(ctx: Context<T, Help>) -> anyhow::Result<()>
        where T: BotClient + BotHandler + std::fmt::Debug + 'static
        {
            const ALL_HELP: &'static str = const_format::concatcp!(
                HelpHandler::HELP_MSG, "\n",
                #( <#cmd_handlers as BuildHelp>::HELP_MSG, "\n", )*
            );
            ctx.send_message_async(crate::abi::message::from_str(ALL_HELP));
            Ok(())
        }

        #[allow(non_snake_case, dead_code)]
        pub fn dispatch_all_handlers<T, M>(context: Context<T, M>)
        where
            T: BotClient + BotHandler + std::fmt::Debug + Sync + Send + 'static,
            M: MessageType + std::fmt::Debug + Sync + Send + 'static,
        {
            let msg_type = context.message.get_type();
            let text = context.get_message_text();

            match msg_type {
                Type::Message => {
                    let prefix = config::get_command_prefix();
                    let prefix_len = prefix.len();

                    'matching_cmd: {
                        if text.starts_with(prefix) {
                            let cmd_part = &text[prefix_len..];

                            #(
                                if cmd_part.starts_with(<#all_cmds as Handler<T, M>>::FILTER_CMD.unwrap()) {
                                    let _ = <#all_cmds as Handler<T, M>>::handle(&#all_cmds, &context);
                                    break 'matching_cmd;
                                }
                            )*
                        }
                    }

                    #(
                        if <#other_handlers as Handler<T, M>>::FILTER_TYPE == Some(Type::Message) {
                             let _ = <#other_handlers as Handler<T, M>>::handle(&#other_handlers, &context);
                        }
                    )*
                }

                Type::Notice => {
                    #(
                        if <#other_handlers as Handler<T, M>>::FILTER_TYPE == Some(Type::Notice) {
                             let _ = <#other_handlers as Handler<T, M>>::handle(&#other_handlers, &context);
                        }
                    )*
                }

                Type::Request => {
                    #(
                        if <#other_handlers as Handler<T, M>>::FILTER_TYPE == Some(Type::Request) {
                             let _ = <#other_handlers as Handler<T, M>>::handle(&#other_handlers, &context);
                        }
                    )*
                }
            }

            #(
                if <#other_handlers as Handler<T, M>>::FILTER_TYPE.is_none() {
                     let _ = <#other_handlers as Handler<T, M>>::handle(&#other_handlers, &context);
                }
            )*
        }
    };

    TokenStream::from(expanded)
}

enum WrapState {
    Normal,
    Query,
}

enum CallType {
    Get,
    Post,
}

struct JwApiArgs {
    url: String,
    app: String,
    field_name: String,
    wrapper_name: String,
    wrap_response: WrapState,
    auto_row: bool,
    call_type: CallType,
}

impl Parse for JwApiArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let vars = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        let mut url: Option<String> = None;
        let mut app: Option<String> = None;
        let mut wrap_response = WrapState::Normal;
        let mut wrapper_name: Option<String> = None;
        let mut auto_row = true;
        let mut call_type = CallType::Post;

        for meta in vars {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("url") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = nv.value
                    {
                        url = Some(s.value());
                    }
                } else if nv.path.is_ident("app") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = nv.value
                    {
                        app = Some(s.value());
                    }
                } else if nv.path.is_ident("wrapper_name") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = nv.value
                    {
                        wrapper_name = Some(s.value());
                    }
                } else if nv.path.is_ident("auto_row") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Bool(b),
                        ..
                    }) = nv.value
                    {
                        auto_row = b.value;
                    }
                } else if nv.path.is_ident("call_type") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = nv.value
                    {
                        let val = s.value();
                        match val.as_str() {
                            "GET" | "get" => call_type = CallType::Get,
                            "POST" | "post" => call_type = CallType::Post,
                            _ => {
                                return Err(syn::Error::new_spanned(
                                    nv.path,
                                    "Invalid value for 'call_type', expected 'GET' or 'POST'",
                                ));
                            }
                        }
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        nv.path,
                        "Unknown attribute key, expected 'url', 'app', 'wrap_response', 'wrapper_name', 'auto_row', or 'call_type'",
                    ));
                }
            }
        }

        let url = url.ok_or_else(|| {
            syn::Error::new(
                input.span(),
                "Missing required attribute: `url` (e.g., #[jw_api(url = \"...\")])",
            )
        })?;

        let app = app.ok_or_else(|| {
            syn::Error::new(
                input.span(),
                "Missing required attribute: `app` (e.g., #[jw_api(app = \"...\")])",
            )
        })?;

        if !url.ends_with(".do") {
            return Err(syn::Error::new(
                input.span(),
                "The url is invalid because it does not end with .do",
            ));
        }

        let field_name = url
            .split('/')
            .next_back()
            .and_then(|s| s.split('.').next())
            .ok_or(syn::Error::new(
                input.span(),
                "The url may be invalid because it does not contain a valid api name",
            ))?
            .to_string();

        if field_name.contains("Xs") {
            wrap_response = WrapState::Query;
        }

        let wrapper_name = match wrapper_name {
            Some(name) => name,
            None => match wrap_response {
                WrapState::Normal => "datas".to_string(),
                WrapState::Query => "data".to_string(),
            },
        };

        Ok(JwApiArgs {
            url,
            app,
            field_name,
            wrap_response,
            wrapper_name,
            auto_row,
            call_type,
        })
    }
}

#[proc_macro_attribute]
pub fn jw_api(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as JwApiArgs);

    let input_struct = parse_macro_input!(input as ItemStruct);
    let original_ident = &input_struct.ident;

    let response_item_ident = format_ident!("{}Response", original_ident);
    let data_api_ident = format_ident!("{}DataApi", original_ident);
    let datas_ident = format_ident!("{}Datas", original_ident);

    let data_name = format_ident!("{}", args.wrapper_name);

    let vis = &input_struct.vis;
    let fields = &input_struct.fields;
    let url_val = args.url;
    let app_val = args.app;

    let field_name_from_url = args.field_name;

    let dynamic_field_ident = format_ident!("{}", field_name_from_url);

    let field_quote = if args.auto_row {
        quote! {
            #[derive(Serialize, Deserialize, Debug, Clone, Default)]
            #[serde(rename_all = "UPPERCASE")]
            #vis struct #response_item_ident
            #fields

            #[derive(Serialize, Deserialize, Debug, Clone, Default)]
            #[serde(rename_all = "camelCase")]
            #vis struct #data_api_ident {
                pub rows: Vec<#response_item_ident>,
                // pub ext_params: serde::de::IgnoredAny,
                // pub page_number: serde::de::IgnoredAny,
                // pub page_size: serde::de::IgnoredAny,
                // pub total_size: serde::de::IgnoredAny,
            }
        }
    } else {
        quote! {
            #[derive(Serialize, Deserialize, Debug, Clone, Default)]
            #[serde(rename_all = "camelCase")]
            #vis struct #data_api_ident
            #fields
        }
    };

    let trait_quote = quote! {
            #[async_trait::async_trait]
            impl JwAPI for #original_ident {
                const URL_DATA: &'static str = #url_val;
                const APP_ENTRANCE: &'static str = #app_val;
            }
    };

    let original_quote = match args.wrap_response {
        WrapState::Normal => quote! {
            #[derive(Serialize, Deserialize, Debug, Clone, Default)]
            #vis struct #datas_ident {
                pub #dynamic_field_ident: #data_api_ident,
            }

            #[derive(Serialize, Deserialize, Debug, Clone, Default)]
            #vis struct #original_ident {
                pub code: String,
                pub #data_name: #datas_ident,
            }
        },
        WrapState::Query => {
            if args.auto_row {
                quote! {
                    #[derive(Serialize, Deserialize, Debug, Clone, Default)]
                    #vis struct #original_ident {
                        pub #data_name: Vec<#response_item_ident>,
                    }
                }
            } else {
                quote! {
                    #[derive(Serialize, Deserialize, Debug, Clone, Default)]
                    #vis struct #original_ident #fields
                }
            }
        }
    };

    let impl_quote = match args.call_type {
        CallType::Get => quote! {
            impl #original_ident {
                pub async fn call_client(client: &crate::api::network::SessionClient) -> Result<#original_ident> {
                    let res_auth = client.get(#original_ident::APP_ENTRANCE).await?;
                    let resp = client.get(#original_ident::URL_DATA).await?.json_smart().await?;
                    Ok(resp)
                }

                pub async fn call(castgc: &str) -> Result<#original_ident> {
                    let client = crate::api::xmu_service::jw::get_castgc_client(castgc);
                    Self::call_client(&client).await
                }
            }
        },
        CallType::Post => quote! {
            impl #original_ident {
                pub async fn call_client<D: Serialize + Sync>(client: &crate::api::network::SessionClient, data: &D) -> Result<#original_ident> {
                    let res_auth = client.get(#original_ident::APP_ENTRANCE).await?;
                    let resp = client.post(#original_ident::URL_DATA, data).await?.json_smart().await?;
                    Ok(resp)
                }

                pub async fn call<D: Serialize + Sync>(castgc: &str, data: &D) -> Result<#original_ident> {
                    let client = crate::api::xmu_service::jw::get_castgc_client(castgc);
                    Self::call_client(&client, data).await
                }
            }
        },
    };

    let expanded = quote! {
        #field_quote

        #trait_quote

        #original_quote

        #impl_quote
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn lnt_get_api(args: TokenStream, input: TokenStream) -> TokenStream {
    // 1. 解析参数：#[lnt_get_api(ResponseType, "url")]
    let arg_parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let args = parse_macro_input!(args with arg_parser);

    if args.len() < 2 {
        panic!(
            "\n[Macros Error]: Expected at least 2 arguments: #[lnt_get_api(ResponseType, \"url\")]\n"
        );
    }

    let response_type = &args[0];
    let url_expr = &args[1];

    // 提取 URL 字符串并获取其原始 Span 用于错误定位和高亮
    let url_string = if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(s),
        ..
    }) = url_expr
    {
        s.value()
    } else {
        panic!("\n[Macros Error]: The second argument must be a string literal (the URL).\n");
    };

    let mut fn_params = Vec::new();
    let mut call_args = Vec::new();
    let mut clean_url = url_string.clone();

    // 2. 解析占位符 {name:Type}

    for (start, _) in url_string.match_indices('{') {
        if let Some(rel_end) = url_string[start..].find('}') {
            let end = start + rel_end;
            let capture = &url_string[start + 1..end]; // 例如 "course_id:i64"

            if let Some((name_str, ty_str)) = capture.split_once(':') {
                let name = name_str.trim();
                let ty_s = ty_str.trim();

                // 编译期类型存在性检查：如果类型写错，编译器红线会画在宏参数上
                let ty: Type = syn::parse_str(ty_s).unwrap_or_else(|_| {
                    panic!("\n[Macros Error]: The type '{}' for parameter '{}' is not a valid Rust type.\n", ty_s, name);
                });

                let name_ident = format_ident!("{}", name, span = url_expr.span());
                fn_params.push(quote! { #name_ident: #ty });
                call_args.push(quote! { #name_ident });

                // 语法糖还原：将 {id:i64} 替换为 {id}，以便标准 format! 识别
                let from = format!("{}:{}", name_str, ty_str);
                clean_url = clean_url.replace(&from, name);
            } else {
                panic!(
                    "\n[Macros Error]: Invalid placeholder format in URL. Expected '{{name:Type}}', found '{{{}}}'.\n",
                    capture
                );
            }
        }
    }

    // 3. 准备生成的字面量和结构体
    let clean_url_lit = LitStr::new(&clean_url, url_expr.span());
    let input_struct = parse_macro_input!(input as ItemStruct);
    let struct_name = &input_struct.ident;

    let url_builder = if call_args.is_empty() {
        // 情况 A：没有参数，直接转 String，避免 format! 损耗
        quote! { #clean_url_lit.to_string() }
    } else {
        // 情况 B：有参数，使用显式绑定 key = value 消除 redundant 警告并支持高亮
        quote! { format!(#clean_url_lit, #(#call_args = #call_args),*) }
    };

    // 4. 生成代码：关联 Span 以实现 IDE 高亮和精准报错
    let expanded = quote_spanned! { url_expr.span() =>
        #input_struct

        impl #struct_name {
            #[allow(dead_code)]
            pub async fn get(session: &str, #(#fn_params),*) -> anyhow::Result<#response_type> {
                let client = crate::api::xmu_service::lnt::get_session_client(session);
                Self::get_from_client(&client, #(#call_args),*).await
            }

            pub async fn get_from_client(client:&crate::api::network::SessionClient, #(#fn_params),*) -> anyhow::Result<#response_type> {
                // 2. 构造 URL (IDE 会在此处通过 Span 关联实现高亮)
                let target_url = #url_builder;

                // 3. 执行请求并处理分级英文错误
                let res = client.get(&target_url).await
                    .map_err(|e| anyhow::anyhow!("Network Error: Failed to reach '{}'. Details: {}", target_url, e))?;

                if !res.status().is_success() {
                    return Err(anyhow::anyhow!(
                        "HTTP Error: API returned status {} for URL: {}",
                        res.status(),
                        target_url
                    ));
                }

                // 4. 反序列化
                let data = res.json_smart::<#response_type>().await
                    .map_err(|e| anyhow::anyhow!(
                        "Deserialization Error: Failed to parse {} from {}. Error: {}",
                        stringify!(#response_type),
                        target_url,
                        e
                    ))?;

                Ok(data)
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn session_client_helper(_args: TokenStream, input: TokenStream) -> TokenStream {
    // 1. 解析输入的函数
    let input_fn = parse_macro_input!(input as ItemFn);
    let sig = &input_fn.sig;
    let old_name = sig.ident.to_string();
    let suffix = "_from_client";

    // 2. 校验后缀：必须以 _from_client 结尾
    if !old_name.ends_with(suffix) {
        return TokenStream::from(quote_spanned! {
            sig.ident.span() => compile_error!("Function name must end with '_from_client' (e.g., 'get_from_client')");
        });
    }

    // 3. 生成新函数名：去掉 "_from_client"
    // 例如 "get_from_client" -> "get"
    let new_name_str = &old_name[..old_name.len() - suffix.len()];

    // 如果去掉后缀后名字为空（比如函数名就叫 _from_client），给个默认名或报错
    if new_name_str.is_empty() {
        return TokenStream::from(quote_spanned! {
            sig.ident.span() => compile_error!("Function name is too short after removing '_from_client'");
        });
    }

    let new_name = format_ident!("{}", new_name_str, span = sig.ident.span());

    // 4. 提取签名要素
    let vis = &input_fn.vis;
    let generics = &sig.generics;
    let where_clause = &generics.where_clause;
    let return_type = &sig.output;

    // 5. 处理参数：保留除第一个 (&SessionClient) 之外的所有参数
    let mut inputs_iter = sig.inputs.iter();
    let first_arg = inputs_iter.next();

    let is_arc = match first_arg {
        Some(FnArg::Typed(pat_type)) => match &*pat_type.ty {
            // 匹配 &SessionClient (引用类型)
            syn::Type::Reference(ty_ref) => {
                if let syn::Type::Path(tp) = &*ty_ref.elem {
                    // 检查路径最后一段是否是 SessionClient
                    let last_seg = tp.path.segments.last().unwrap();
                    if last_seg.ident == "SessionClient" {
                        false
                    } else {
                        return TokenStream::from(quote_spanned! {
                            pat_type.ty.span() => compile_error!("the type of client should be the 'SessionClient'");
                        });
                    }
                } else {
                    return TokenStream::from(quote_spanned! {
                        pat_type.ty.span() => compile_error!("the first argument must be '&SessionClient'");
                    });
                }
            }
            // 匹配 Arc<SessionClient> (路径类型)
            syn::Type::Path(ty_path) => {
                let last_seg = ty_path.path.segments.last().unwrap();
                if last_seg.ident == "Arc" {
                    // 进一步校验泛型参数是否为 SessionClient
                    let mut valid_inner = false;
                    if let syn::PathArguments::AngleBracketed(args) = &last_seg.arguments
                        && let Some(syn::GenericArgument::Type(syn::Type::Path(inner_tp))) =
                            args.args.first()
                        && inner_tp.path.segments.last().map(|s| &s.ident)
                            == Some(&format_ident!("SessionClient"))
                    {
                        valid_inner = true;
                    }
                    if valid_inner {
                        true
                    } else {
                        return TokenStream::from(quote_spanned! {
                            pat_type.ty.span() => compile_error!("the type inside 'Arc' must be 'SessionClient'");
                        });
                    }
                } else {
                    return TokenStream::from(quote_spanned! {
                        pat_type.ty.span() => compile_error!("the first argument must be 'Arc<SessionClient>' or '&SessionClient'");
                    });
                }
            }
            _ => {
                return TokenStream::from(quote_spanned! {
                    pat_type.ty.span() => compile_error!("Unsupported type for client argument");
                });
            }
        },

        _ => {
            return TokenStream::from(quote_spanned! {
                sig.span() => compile_error!("Function must have at least one argument like: client: &SessionClient");
            });
        }
    };

    let other_params: Vec<_> = inputs_iter.collect();

    // 提取用于调用的参数标识符 (例如 prompt)
    let call_args: Vec<_> = other_params
        .iter()
        .map(|arg| {
            if let FnArg::Typed(pat_type) = arg
                && let Pat::Ident(pat_ident) = &*pat_type.pat
            {
                let id = &pat_ident.ident;
                return quote! { #id };
            }
            quote! { _ }
        })
        .collect();

    // 6. 生成新函数
    let client_invocation = if is_arc {
        // 如果原函数要 Arc，我们把 get_session_client 返回的引用包装成新的 Arc
        // 假设 get_session_client 返回的是 SessionClient 实例
        quote! { Arc::new(client) }
    } else {
        // 如果原函数只要引用，维持原状
        quote! { &client }
    };

    let original_ident = &sig.ident;
    let gen_fn = quote_spanned! { sig.span() =>
        #vis async fn #new_name #generics (
            session: &str,
            #(#other_params),*
        ) #return_type #where_clause {
            // 初始化 Client 并设置 Cookie
            let client = crate::api::xmu_service::lnt::get_session_client(session);

            // 内部调用原始的 _from_client 函数
            Self::#original_ident(#client_invocation, #(#call_args),*).await
        }
    };

    // 7. 合并输出：保留原函数，追加新函数
    let expanded = quote! {
        #input_fn
        #gen_fn
    };

    TokenStream::from(expanded)
}

// 定义解析输入的数据结构
struct BoxNewInput {
    struct_type: Type,
    _comma: Token![,],
    data: BoxData,
}

enum BoxData {
    // 处理 { field: value, ... }
    Struct(Punctuated<FieldValue, Token![,]>),
    // 处理 Enum::Variant(val) 或 变量
    Expr(Expr),
}

struct FieldValue {
    member: Ident,
    _colon: Token![:],
    value: Expr,
}

impl Parse for BoxNewInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let struct_type: Type = input.parse()?;
        let comma: Token![, ] = input.parse()?;

        if input.peek(token::Brace) {
            let content;
            let _brace = syn::braced!(content in input);
            let fields = content.parse_terminated(FieldValue::parse, Token![,])?;
            Ok(BoxNewInput {
                struct_type,
                _comma: comma,
                data: BoxData::Struct(fields),
            })
        } else {
            let expr: Expr = input.parse()?;
            Ok(BoxNewInput {
                struct_type,
                _comma: comma,
                data: BoxData::Expr(expr),
            })
        }
    }
}

impl Parse for FieldValue {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(FieldValue {
            member: input.parse()?,
            _colon: input.parse()?,
            value: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn box_new(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as BoxNewInput);
    let ty = &input.struct_type;

    match input.data {
        BoxData::Struct(fields) => {
            let f_names: Vec<_> = fields.iter().map(|f| &f.member).collect();
            let f_values: Vec<_> = fields.iter().map(|f| &f.value).collect();

            quote! {
                {
                    // 1. 预先计算值（url.clone() 在这里发生，存入局部变量）
                    #( let #f_names = #f_values; )*

                    // 2. 核心检查：改用解构模式匹配 (Destructuring Pattern)
                    // 这种方式只检查字段全不全，不涉及变量所有权的转移（Move）
                    if false {
                        #[allow(unreachable_code, unused_variables)]
                        {
                            // 只要 #f_names 漏掉了字段，这里解构 #ty 就会编译报错
                            let #ty { #( #f_names: _ ),* } = unsafe { std::mem::zeroed::<#ty>() };
                        }
                    }

                    // 3. 真正的堆内存分配和写入
                    let mut b = Box::<#ty>::new_uninit();
                    let ptr = b.as_mut_ptr();
                    unsafe {
                        #(
                            // 这里才真正发生 Move，把局部变量写入堆
                            std::ptr::addr_of_mut!((*ptr).#f_names).write(#f_names);
                        )*
                        b.assume_init()
                    }
                }
            }
            .into()
        }
        BoxData::Expr(expr) => quote! {
            {
                let val = #expr;
                let mut b = Box::<#ty>::new_uninit();
                unsafe {
                    b.as_mut_ptr().write(val);
                    b.assume_init()
                }
            }
        }
        .into(),
    }
}

#[proc_macro_attribute]
pub fn castgc_client_helper(_args: TokenStream, input: TokenStream) -> TokenStream {
    // 1. 解析输入的函数
    let input_fn = parse_macro_input!(input as ItemFn);
    let sig = &input_fn.sig;
    let old_name = sig.ident.to_string();
    let suffix = "_from_client";

    // 2. 校验后缀
    if !old_name.ends_with(suffix) {
        return TokenStream::from(quote_spanned! {
            sig.ident.span() => compile_error!("Function name must end with '_from_client'");
        });
    }

    // 3. 生成新函数名 (去掉后缀)
    let new_name_str = &old_name[..old_name.len() - suffix.len()];
    if new_name_str.is_empty() {
        return TokenStream::from(quote_spanned! {
            sig.ident.span() => compile_error!("Function name invalid");
        });
    }
    let new_name = format_ident!("{}", new_name_str, span = sig.ident.span());

    // 4. 提取签名要素
    let vis = &input_fn.vis;
    let generics = &sig.generics;
    let where_clause = &generics.where_clause;
    let return_type = &sig.output;

    // 5. 处理参数：校验第一个参数是否为 &SessionClient 或 Arc<SessionClient>
    let mut inputs_iter = sig.inputs.iter();
    let first_arg = inputs_iter.next();

    let is_arc = match first_arg {
        Some(FnArg::Typed(pat_type)) => match &*pat_type.ty {
            // 匹配 &SessionClient
            syn::Type::Reference(ty_ref) => {
                if let syn::Type::Path(tp) = &*ty_ref.elem {
                    let last_seg = tp.path.segments.last().unwrap();
                    if last_seg.ident == "SessionClient" {
                        false
                    } else {
                        return TokenStream::from(quote_spanned! {
                            pat_type.ty.span() => compile_error!("First arg must be '&SessionClient'");
                        });
                    }
                } else {
                    return TokenStream::from(quote_spanned! {
                        pat_type.ty.span() => compile_error!("First arg must be '&SessionClient'");
                    });
                }
            }
            // 匹配 Arc<SessionClient>
            syn::Type::Path(ty_path) => {
                let last_seg = ty_path.path.segments.last().unwrap();
                if last_seg.ident == "Arc" {
                    let mut valid_inner = false;
                    if let syn::PathArguments::AngleBracketed(args) = &last_seg.arguments {
                        // 进一步校验泛型参数是否为 SessionClient
                        if let Some(syn::GenericArgument::Type(syn::Type::Path(inner_tp))) =
                            args.args.first()
                        {
                            // 检查内部类型是否为 SessionClient
                            if inner_tp.path.segments.last().map(|s| &s.ident)
                                == Some(&format_ident!("SessionClient"))
                            {
                                valid_inner = true;
                            }
                        }
                    }
                    if valid_inner {
                        true
                    } else {
                        return TokenStream::from(quote_spanned! {
                            pat_type.ty.span() => compile_error!("Arc must contain 'SessionClient'");
                        });
                    }
                } else {
                    return TokenStream::from(quote_spanned! {
                        pat_type.ty.span() => compile_error!("First arg must be 'Arc<SessionClient>' or '&SessionClient'");
                    });
                }
            }
            _ => {
                return TokenStream::from(quote_spanned! {
                    pat_type.ty.span() => compile_error!("Unsupported client type");
                });
            }
        },
        _ => {
            return TokenStream::from(quote_spanned! {
                sig.span() => compile_error!("Function needs client argument");
            });
        }
    };

    let other_params: Vec<_> = inputs_iter.collect();

    // 提取后续调用参数标识符
    let call_args: Vec<_> = other_params
        .iter()
        .map(|arg| {
            if let FnArg::Typed(pat_type) = arg {
                // 提取参数标识符
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let id = &pat_ident.ident;
                    return quote! { #id };
                }
            }
            quote! { _ }
        })
        .collect();

    // 6. 生成新函数逻辑
    let client_invocation = if is_arc {
        quote! { std::sync::Arc::new(client) }
    } else {
        quote! { &client }
    };

    let original_ident = &sig.ident;

    let gen_fn = quote_spanned! { sig.span() =>
        #vis async fn #new_name #generics (
            castgc: &str, // 替换为 castgc 参数
            #(#other_params),*
        ) #return_type #where_clause {
            // 调用你指定的内部生成方法
            let client = crate::api::xmu_service::jw::get_castgc_client(castgc);

            // 调用原始函数
            Self::#original_ident(#client_invocation, #(#call_args),*).await
        }
    };

    // 7. 合并输出
    let expanded = quote! {
        #input_fn
        #gen_fn
    };

    TokenStream::from(expanded)
}

struct ToolAttr {
    name: Option<String>,
    description: String,
}

impl Parse for ToolAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut name = None;
        let mut description = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;

            match ident.to_string().as_str() {
                "name" => name = Some(value.value()),
                "description" => description = Some(value.value()),
                _ => {
                    return Err(syn::Error::new(
                        ident.span(),
                        "unknown tool attribute (expected `name` or `description`)",
                    ));
                }
            }

            let _ = input.parse::<Token![,]>();
        }

        Ok(Self {
            name,
            description: description
                .ok_or_else(|| syn::Error::new(input.span(), "`description` is required"))?,
        })
    }
}

#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = parse_macro_input!(attr as ToolAttr);
    let mut func = parse_macro_input!(item as ItemFn);

    match expand_tool(attr, &mut func) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_tool(attr: ToolAttr, func: &mut ItemFn) -> Result<proc_macro2::TokenStream> {
    let fn_name = func.sig.ident.clone();
    let fn_name_str = attr.name.unwrap_or_else(|| fn_name.to_string());

    let tool_fn_name = format_ident!("{}_tool", fn_name);
    let tool_struct = format_ident!(
        "{}Tool",
        AsPascalCase(fn_name.to_string()).to_string(),
        span = fn_name.span()
    );
    let args_struct = format_ident!(
        "{}Args",
        AsPascalCase(fn_name.to_string()).to_string(),
        span = fn_name.span()
    );

    // -------- parameters --------
    let mut arg_fields = vec![];
    let mut arg_names = vec![];
    let mut schema_assignments = vec![];
    let mut required = vec![];

    for arg in &mut func.sig.inputs {
        let FnArg::Typed(pat) = arg else {
            return Err(syn::Error::new_spanned(arg, "self not allowed"));
        };

        let ident = match &*pat.pat {
            syn::Pat::Ident(id) => id.ident.clone(),
            _ => return Err(syn::Error::new_spanned(&pat.pat, "unsupported pattern")),
        };

        let doc = extract_doc(&pat.attrs);

        // 清除参数上的属性（如文档注释），因为 Rust 不允许在函数参数上使用自定义或文档属性
        pat.attrs.clear();

        let ty = pat.ty.clone();
        let doc_attr = match &doc {
            Some(doc) => quote! { #[doc = #doc] },
            None => quote! {},
        };

        let schema = match &doc {
            Some(doc) => quote! {
                <#ty as ToolStruct>::schema_with_override(Some(#doc))
            },
            None => quote! {
                <#ty as ToolStruct>::tool_schema()
            },
        };

        arg_fields.push(quote! {
            #doc_attr
            pub #ident: #ty
        });
        schema_assignments.push(schema);
        if !is_option(&ty) {
            required.push(ident.clone());
        }
        arg_names.push(ident);
    }

    // 检查返回值类型是否为 Result<String, ...>
    let mut valid_return = false;
    if let syn::ReturnType::Type(_, ty) = &func.sig.output
        && let syn::Type::Path(tp) = ty.as_ref()
        && let Some(seg) = tp.path.segments.last()
        && seg.ident == "Result"
        && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
        && let Some(syn::GenericArgument::Type(syn::Type::Path(inner_tp))) = args.args.first()
        && inner_tp.path.is_ident("String")
    {
        valid_return = true;
    }
    if !valid_return {
        return Err(syn::Error::new_spanned(
            &func.sig.output,
            "tool function must return `anyhow::Result<String>`",
        ));
    }

    let description = attr.description;

    let schema_json = quote! {
        serde_json::json!({
            "type": "object",
            "properties": {
                #(
                    stringify!(#arg_names): #schema_assignments,
                )*
            },
            "required": [#(stringify!(#required)),*],
        })
    };

    Ok(quote! {
        use crate::api::llm::chat::tool::{ToolCallback, ToolStruct};
        use genai::chat::Tool;

        #func

        #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
        pub struct #args_struct {
            #(#arg_fields,)*
        }

        pub struct #tool_struct;

        #[async_trait::async_trait]
        impl ToolCallback for #tool_struct {
            const FN_NAME: &'static str = #fn_name_str;

            type Output = #args_struct;

            async fn call(args: Self::Output) -> anyhow::Result<String> {
                #fn_name(#(args.#arg_names),*).await
            }

            fn tool() -> Tool {
                #tool_fn_name()
            }
        }

        pub fn #tool_fn_name() -> Tool {
            Tool::new(#fn_name_str)
                .with_description(#description)
                .with_schema(#schema_json)
        }
    })
}

fn is_option(ty: &Type) -> bool {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
        && seg.ident == "Option"
    {
        return true;
    }
    false
}

fn extract_doc(attrs: &[Attribute]) -> Option<String> {
    let mut docs = vec![];

    for attr in attrs {
        if attr.path().is_ident("doc")
            && let Meta::NameValue(meta) = &attr.meta
            && let Expr::Lit(expr) = &meta.value
            && let Lit::Str(s) = &expr.lit
        {
            docs.push(s.value());
        }
    }

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}
