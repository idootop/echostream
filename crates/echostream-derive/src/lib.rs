//! EchoStream 过程宏：`#[rpc]` / `#[event]` / `#[stream]`
//!
//! 将普通 `async fn` 转换为框架 Handler（零大小类型），自动完成参数提取，
//! 业务代码只面对强类型参数，无需手动处理编解码。

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, PatType, ReturnType, Type, parse_macro_input};

/// 将函数标记为 RPC 处理器
///
/// Session 参数可写可不写（不写则自动省略，处理器内不需要会话时最简洁）：
/// - `async fn(&Session, Req) -> Result<Resp>` —— 需要会话
/// - `async fn(Req) -> Result<Resp>` —— 省略会话
/// - `async fn(&Session) -> Result<Resp>` —— 无请求参数
/// - `async fn() -> Result<Resp>` —— 最简形式
///
/// 属性可指定方法名：`#[rpc("user.login")]`，默认使用函数名。
#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand(HandlerKind::Rpc, attr, item)
}

/// 将函数标记为事件处理器
///
/// Session 参数同样可省略（见 `#[rpc]`），返回 `Result<()>` 或 `()`：
/// - `async fn(&Session, Data)` / `async fn(Data)`
/// - `async fn(&Session)` / `async fn()` —— 无载荷事件
#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand(HandlerKind::Event, attr, item)
}

/// 将函数标记为流处理器
///
/// 必须包含 `StreamReceiver` 参数；Session 参数可省略：
/// - `async fn(&Session, StreamReceiver) -> Result<()>`
/// - `async fn(StreamReceiver) -> Result<()>`
#[proc_macro_attribute]
pub fn stream(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand(HandlerKind::Stream, attr, item)
}

// ======================== 展开逻辑 ========================

#[derive(Clone, Copy)]
enum HandlerKind {
    Rpc,
    Event,
    Stream,
}

fn expand(kind: HandlerKind, attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);

    // 校验：必须为 async fn
    if func.sig.asyncness.is_none() {
        return err("EchoStream 处理器必须是 async fn");
    }
    // 不支持泛型（保持简单）
    if !func.sig.generics.params.is_empty() {
        return err("EchoStream 处理器不支持泛型参数");
    }

    let fn_ident = &func.sig.ident;
    let handler_name = parse_name(attr, fn_ident);
    let struct_ident = to_struct_name(fn_ident);

    // 提取 Session 参数与数据参数
    let mut session_arg: Option<&PatType> = None;
    let mut data_arg: Option<&PatType> = None;
    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat) = arg {
            if is_session_type(&pat.ty) {
                if session_arg.is_some() {
                    return err("Session 参数只能出现一次");
                }
                session_arg = Some(pat);
            } else if data_arg.is_none() {
                data_arg = Some(pat);
            } else {
                return err("处理器最多支持一个数据参数");
            }
        } else {
            return err("处理器不支持 self / 变长参数");
        }
    }

    let session = session_arg.map(|p| p.ty.as_ref().clone());
    let data = data_arg.map(|p| p.ty.as_ref().clone());

    match kind {
        HandlerKind::Rpc => expand_rpc(func, handler_name, struct_ident, session, data),
        HandlerKind::Event => expand_event(func, handler_name, struct_ident, session, data),
        HandlerKind::Stream => expand_stream(func, handler_name, struct_ident, session, data),
    }
}

/// 展开 RPC 处理器
fn expand_rpc(
    func: ItemFn,
    handler_name: String,
    struct_ident: syn::Ident,
    session: Option<Type>,
    data: Option<Type>,
) -> TokenStream {
    // 解析返回类型：Result<Resp> 或 Resp
    let (resp_ty, is_result) = parse_return(&func.sig.output);

    // 构建业务调用
    let invoke = build_invoke("rpc", &func.sig.ident, &session, &data, is_result);

    let req_ty = data.clone().unwrap_or_else(|| syn::parse_quote!(()));
    // trait 签名固定（session + req），缺省参数用下划线避免 unused 警告
    let (session_param, data_param) = impl_params("rpc", &session, &data);

    let expanded = quote! {
        #func

        /// EchoStream RPC 处理器（由 `#[rpc]` 宏生成）
        pub struct #struct_ident;

        #[::echostream::async_trait]
        impl ::echostream::RpcHandler for #struct_ident {
            type Req = #req_ty;
            type Resp = #resp_ty;

            fn name(&self) -> &str {
                #handler_name
            }

            async fn handle(&self, #session_param #data_param) -> ::echostream::Result<Self::Resp> {
                #invoke
            }
        }
    };
    expanded.into()
}

/// 展开事件处理器
fn expand_event(
    func: ItemFn,
    handler_name: String,
    struct_ident: syn::Ident,
    session: Option<Type>,
    data: Option<Type>,
) -> TokenStream {
    // 事件处理器返回值：Result<()> 或 ()
    let (ret_ty, is_result) = parse_return(&func.sig.output);
    let ret_is_unit = matches!(ret_ty, Type::Tuple(t) if t.elems.is_empty());
    if !ret_is_unit && !is_result {
        return err("事件处理器返回值必须是 Result<()> 或 ()");
    }

    let invoke = build_invoke("event", &func.sig.ident, &session, &data, is_result);

    let data_ty = data.clone().unwrap_or_else(|| syn::parse_quote!(()));
    // trait 签名固定（session + data），缺省参数用下划线避免 unused 警告
    let (session_param, data_param) = impl_params("event", &session, &data);

    let expanded = quote! {
        #func

        /// EchoStream 事件处理器（由 `#[event]` 宏生成）
        pub struct #struct_ident;

        #[::echostream::async_trait]
        impl ::echostream::EventHandler for #struct_ident {
            type Data = #data_ty;

            fn name(&self) -> &str {
                #handler_name
            }

            async fn handle(&self, #session_param #data_param) -> ::echostream::Result<()> {
                #invoke
                Ok(())
            }
        }
    };
    expanded.into()
}

/// 展开流处理器
fn expand_stream(
    func: ItemFn,
    handler_name: String,
    struct_ident: syn::Ident,
    session: Option<Type>,
    data: Option<Type>,
) -> TokenStream {
    // 校验数据参数必须是 StreamReceiver
    if let Some(ty) = &data {
        if !type_last_segment(ty, "StreamReceiver") {
            return err("流处理器的数据参数必须是 StreamReceiver");
        }
    } else {
        return err("流处理器必须包含 StreamReceiver 参数");
    }

    let (_, is_result) = parse_return(&func.sig.output);
    let invoke = build_invoke("stream", &func.sig.ident, &session, &data, is_result);

    // trait 签名固定（session + stream），缺省 session 用下划线避免 unused 警告
    let (session_param, _) = impl_params("stream", &session, &data);

    let expanded = quote! {
        #func

        /// EchoStream 流处理器（由 `#[stream]` 宏生成）
        pub struct #struct_ident;

        #[::echostream::async_trait]
        impl ::echostream::StreamHandler for #struct_ident {
            fn name(&self) -> &str {
                #handler_name
            }

            async fn handle(&self, #session_param stream: ::echostream::StreamReceiver) -> ::echostream::Result<()> {
                #invoke
                Ok(())
            }
        }
    };
    expanded.into()
}

// ======================== 辅助函数 ========================

/// 生成 impl 的 handle 参数（trait 签名固定为 session + 数据参数）
///
/// 用户函数缺省 session / 数据参数时，生成的 impl 参数以下划线命名，
/// 既满足 trait 签名又避免 unused 警告。
fn impl_params(
    kind: &str,
    session: &Option<Type>,
    data: &Option<Type>,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let session_param = match session {
        Some(_) => quote! { session: &::echostream::Session, },
        None => quote! { _session: &::echostream::Session, },
    };
    let data_param = match (kind, data) {
        ("rpc", Some(_)) => quote! { req: Self::Req },
        ("rpc", None) => quote! { _req: Self::Req },
        ("event", Some(_)) => quote! { data: Self::Data },
        ("event", None) => quote! { _data: Self::Data },
        // stream：必须含 StreamReceiver（expand 已校验）
        _ => quote! { stream: ::echostream::StreamReceiver },
    };
    (session_param, data_param)
}

/// 构建业务调用语句
fn build_invoke(
    kind: &str,
    fn_ident: &syn::Ident,
    session: &Option<Type>,
    data: &Option<Type>,
    is_result: bool,
) -> proc_macro2::TokenStream {
    // 数据参数名：RPC 为 req、事件为 data、流为 stream（与生成的 handle 签名一致）
    let data_name = match kind {
        "rpc" => quote! { req },
        "event" => quote! { data },
        _ => quote! { stream },
    };
    let call = match (session.is_some(), data.is_some()) {
        (true, true) => quote! { #fn_ident(session, #data_name) },
        (true, false) => quote! { #fn_ident(session) },
        (false, true) => quote! { #fn_ident(#data_name) },
        (false, false) => quote! { #fn_ident() },
    };
    if kind == "rpc" {
        if is_result {
            quote! { #call.await }
        } else {
            quote! { Ok(#call.await?) }
        }
    } else if is_result {
        quote! { #call.await?; }
    } else {
        quote! { #call.await; }
    }
}

/// 解析处理器名称：attr 字符串字面量优先，否则使用函数名
fn parse_name(attr: TokenStream, fn_ident: &syn::Ident) -> String {
    let attr_str = attr.to_string();
    if attr_str.trim().is_empty() {
        fn_ident.to_string()
    } else {
        match syn::parse::<LitStr>(attr) {
            Ok(lit) => lit.value(),
            Err(_) => fn_ident.to_string(),
        }
    }
}

/// 函数名转结构体名：`handle_audio` -> `HandleAudio`
fn to_struct_name(fn_ident: &syn::Ident) -> syn::Ident {
    let name = fn_ident.to_string();
    let mut result = String::with_capacity(name.len());
    let mut capitalize = true;
    for c in name.chars() {
        if c == '_' {
            capitalize = true;
        } else if capitalize {
            result.extend(c.to_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }
    syn::Ident::new(&result, Span::call_site())
}

/// 判断类型是否为 Session（`Session` 或 `&Session`，允许任意模块路径前缀）
fn is_session_type(ty: &Type) -> bool {
    match ty {
        Type::Path(_) => type_last_segment(ty, "Session"),
        Type::Reference(r) => is_session_type(&r.elem),
        _ => false,
    }
}

/// 判断类型路径的最后一段是否为指定名字
fn type_last_segment(ty: &Type, name: &str) -> bool {
    if let Type::Path(p) = ty {
        return p.path.segments.last().is_some_and(|seg| seg.ident == name);
    }
    false
}

/// 解析返回类型：`Result<T>` -> (T, true)；`T` -> (T, false)
fn parse_return(output: &ReturnType) -> (Type, bool) {
    let ty = match output {
        ReturnType::Type(_, ty) => ty.as_ref(),
        ReturnType::Default => return (syn::parse_quote!(()), false),
    };
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "Result"
        && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
        && let Some(syn::GenericArgument::Type(t)) = args.args.first()
    {
        return (t.clone(), true);
    }
    (ty.clone(), false)
}

/// 生成编译错误
fn err(msg: &str) -> TokenStream {
    syn::Error::new(Span::call_site(), msg)
        .to_compile_error()
        .into()
}
