//! cardlibs —— 卡片前端界面「帧内预置库」静态路由（GET /card-libs/<name>）
//!
//! ── 为什么需要 ──
//! 上游酒馆助手对**每个前端帧**注入一套第三方库（jQuery / Vue / YAML …），卡作者是按
//! 「这些全局量存在」来写卡的（官方文档明写「不要自己引 CDN，用帧里已注入好的库」）。
//! 我们此前只注入 TavernHelper 桥，于是依赖 `$` / `Vue` / `jsyaml` 的卡在帧里直接
//! ReferenceError。本模块把同一批库以**构建期内嵌**的方式供给 srcdoc 帧。
//!
//! ── 为什么是本地路由，而不是宿主 blob: ──
//! 我们的帧是 `sandbox` 不含 `allow-same-origin` 的**不透明源**，宿主创建的 blob URL
//! 绑定宿主源、帧读不到（实测：脚本不执行、样式不生效，且**零 CSP 违规** ⇒ 是源检查
//! 而非策略拦截）。上游能用 blob 恰恰因为它的帧同源零沙箱 —— 这是「用安全换兼容」的
//! 对价，不能照抄。故改走只读静态路由。
//!
//! ── 鉴权 ──
//! iframe 的 `<script src>` / `<link href>` 是浏览器发起的子资源请求，**带不了
//! X-Relay-Auth**，因此 `/card-libs/` 必须落在静态豁免段内（见 mod.rs 的
//! `EXEMPT_PREFIXES`，本次由 4 个只读前缀扩为 5 个）。名字走**精确白名单**匹配，
//! 不做路径拼接、不做百分号解码 —— 不构成通用文件读取面。
//!
//! ── 清单维护 ──
//! 版本/许可/校验和登记在 `src-tauri/assets/card-libs/LICENSES.md`；升级 = 换带版本的
//! 文件名 + 同步前端清单（`src/services/st/embed.ts` 的 `CARD_LIB_FILES`）+ 更新该表，
//! 由 `scripts/selfcheck-cardlibs.mjs` 守（前端清单 ↔ 本表 ↔ 磁盘文件三向一致）。

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::apply_cors_resp;

/// 单个内嵌库资源：`name` 即 URL 段（含版本），内容构建期嵌入二进制。
struct Asset {
    name: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

/// 批1 清单（顺序即注入顺序：jQuery → toastr → lodash → js-yaml → Vue → VueRouter）。
/// 文件名内嵌版本 ⇒ 可安全长缓存（见下方 Cache-Control）。
const ASSETS: &[Asset] = &[
    Asset {
        name: "jquery-3.7.1.min.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/jquery-3.7.1.min.js"),
    },
    Asset {
        name: "toastr-2.1.4.min.css",
        mime: "text/css; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/toastr-2.1.4.min.css"),
    },
    Asset {
        name: "toastr-2.1.4.min.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/toastr-2.1.4.min.js"),
    },
    Asset {
        name: "lodash-4.17.21.min.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/lodash-4.17.21.min.js"),
    },
    Asset {
        name: "js-yaml-4.1.0.min.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/js-yaml-4.1.0.min.js"),
    },
    Asset {
        name: "vue-3.5.13.global.prod.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/vue-3.5.13.global.prod.js"),
    },
    Asset {
        name: "vue-router-4.5.0.global.prod.js",
        mime: "application/javascript; charset=utf-8",
        bytes: include_bytes!("../../assets/card-libs/vue-router-4.5.0.global.prod.js"),
    },
];

/// 只读清单（供自检/文档引用；名字与注入顺序一致）。
pub fn names() -> Vec<&'static str> {
    ASSETS.iter().map(|a| a.name).collect()
}

/// GET /card-libs/<name>：精确名字命中则回资源，否则 404。
/// 不做路径拼接/解码 —— 名字必须是清单里的字面量（S3 豁免段的既有口径）。
pub fn serve(path: &str, origin: Option<HeaderValue>) -> Response {
    let name = path
        .strip_prefix("/card-libs/")
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    match ASSETS.iter().find(|a| a.name == name) {
        Some(a) => {
            let resp = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, a.mime)
                // 文件名含版本 ⇒ 内容不可变，可长缓存（端口每次启动不同，缓存不跨启动）
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .header("x-content-type-options", "nosniff")
                .body(Body::from(Bytes::from_static(a.bytes)))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            apply_cors_resp(resp, origin)
        }
        None => apply_cors_resp(StatusCode::NOT_FOUND.into_response(), origin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch1_lists_expected_libraries_in_injection_order() {
        // 批1 成员与顺序（升级/换库必须显式改这里 + LICENSES.md + 前端清单）
        assert_eq!(
            names(),
            vec![
                "jquery-3.7.1.min.js",
                "toastr-2.1.4.min.css",
                "toastr-2.1.4.min.js",
                "lodash-4.17.21.min.js",
                "js-yaml-4.1.0.min.js",
                "vue-3.5.13.global.prod.js",
                "vue-router-4.5.0.global.prod.js",
            ]
        );
    }

    #[test]
    fn serves_every_listed_asset_with_nosniff_and_immutable_cache() {
        for name in names() {
            let r = serve(&format!("/card-libs/{name}"), None);
            assert_eq!(r.status(), StatusCode::OK, "{name} 应 200");
            assert_eq!(
                r.headers()
                    .get("x-content-type-options")
                    .and_then(|v| v.to_str().ok()),
                Some("nosniff"),
                "{name} 必须带 nosniff"
            );
            let cc = r
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(cc.contains("immutable"), "{name} 应可长缓存");
            let ct = r
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(ct.starts_with("application/javascript") || ct.starts_with("text/css"));
        }
    }

    #[test]
    fn unknown_or_traversing_names_are_404() {
        for p in [
            "/card-libs/",
            // 无版本旧名（防回退到未版本化文件名）
            "/card-libs/jquery.min.js",
            "/card-libs/../../secrets.json",
            "/card-libs/..%2f..%2fsecrets.json",
            "/card-libs/jquery-3.7.1.min.js/extra",
            // 不做百分号解码：编码过的名字不匹配
            "/card-libs/%6aquery-3.7.1.min.js",
            "/card-libs/whatever.js",
            // 前缀之外（调用方不该走到这里，兜底仍 404）
            "/card-libs",
            "/characters/x.png",
        ] {
            assert_eq!(
                serve(p, None).status(),
                StatusCode::NOT_FOUND,
                "{p} 应 404"
            );
        }
    }
}
