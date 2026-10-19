//! cardlibs —— 卡片前端界面「帧内预置库」静态路由（GET /card-libs/<name>）
//!
//! ── 为什么需要 ──
//! 上游酒馆助手对**每个前端帧**注入一套第三方库（jQuery / Vue / YAML / Font Awesome …），
//! 卡作者是按「这些全局量存在」来写的（官方文档明写「不要自己引 CDN，用帧里已注入好的库」）。
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
//! `EXEMPT_PREFIXES`，批1 时由 4 个只读前缀扩为 5 个）。名字走**精确白名单**匹配，
//! 不做路径拼接、不做百分号解码 —— 不构成通用文件读取面。
//!
//! ── 两类条目（重要）──
//! - `inject: true`：**要注入帧 `<head>` 的** css/js（前端 `CARD_LIB_FILES` 与之同名同序）。
//! - `inject: false`：**只由 CSS 相对引用**的附属资源（woff2 字体、主题图片）。
//!   它们不注入，只被上一条里的 CSS 以 `../webfonts/…`、`images/…` 拉取，
//!   因此名字带子目录（`serve` 用整名精确匹配，故子路径安全）。
//!
//! ── 清单维护 ──
//! 版本/许可/校验和登记在 `src-tauri/assets/card-libs/LICENSES.md`；升级 = 换带版本的
//! 文件名 + 同步前端清单（`src/services/st/embed.ts` 的 `CARD_LIB_FILES`）+ 更新该表，
//! 由 `scripts/selfcheck-cardlibs.mjs` 守（可注入集 ↔ 前端清单、全表 ↔ 磁盘文件）。

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::apply_cors_resp;

/// 单个内嵌资源：`name` 即 URL 段（= 相对 assets/card-libs 的路径前缀 + 文件名，含版本）。
struct Asset {
    name: &'static str,
    mime: &'static str,
    /// true = 注入帧 head（css/js）；false = 仅由 CSS 相对引用的附属资源（字体/图片）
    inject: bool,
    bytes: &'static [u8],
}

/// 全表（批1 + 批2）。顺序：先批1（jQuery → toastr → lodash → js-yaml → Vue → VueRouter），
/// 再批2（Font Awesome → jQuery UI 主题 → jQuery UI → touch-punch → showdown）。
const ASSETS: &[Asset] = &[
    /* ---------------- 批1 ---------------- */
    Asset {
        name: "jquery-3.7.1.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/jquery-3.7.1.min.js"),
    },
    Asset {
        name: "toastr-2.1.4.min.css",
        mime: "text/css; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/toastr-2.1.4.min.css"),
    },
    Asset {
        name: "toastr-2.1.4.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/toastr-2.1.4.min.js"),
    },
    Asset {
        name: "lodash-4.17.21.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/lodash-4.17.21.min.js"),
    },
    Asset {
        name: "js-yaml-4.1.0.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/js-yaml-4.1.0.min.js"),
    },
    Asset {
        name: "vue-3.5.13.global.prod.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/vue-3.5.13.global.prod.js"),
    },
    Asset {
        name: "vue-router-4.5.0.global.prod.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/vue-router-4.5.0.global.prod.js"),
    },
    /* ---------------- 批2 ---------------- */
    // Font Awesome 6.7.2：保留 `.fa` 兼容类（`fa fa-user` 老写法可用）；只 ship woff2
    // （CSS 的 src 列表 woff2 优先，现代内核必然选它，ttf 兜底不再打包）
    Asset {
        name: "fontawesome-6.7.2/css/all.min.css",
        mime: "text/css; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/fontawesome-6.7.2/css/all.min.css"),
    },
    Asset {
        name: "fontawesome-6.7.2/webfonts/fa-solid-900.woff2",
        mime: "font/woff2",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/fontawesome-6.7.2/webfonts/fa-solid-900.woff2"),
    },
    Asset {
        name: "fontawesome-6.7.2/webfonts/fa-regular-400.woff2",
        mime: "font/woff2",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/fontawesome-6.7.2/webfonts/fa-regular-400.woff2"),
    },
    Asset {
        name: "fontawesome-6.7.2/webfonts/fa-brands-400.woff2",
        mime: "font/woff2",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/fontawesome-6.7.2/webfonts/fa-brands-400.woff2"),
    },
    Asset {
        name: "fontawesome-6.7.2/webfonts/fa-v4compatibility.woff2",
        mime: "font/woff2",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/fontawesome-6.7.2/webfonts/fa-v4compatibility.woff2"),
    },
    // jQuery UI 1.13.3：主题 CSS 引 images/ 下 6 张图标精灵
    Asset {
        name: "jquery-ui-1.13.3/themes/base/theme.css",
        mime: "text/css; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/theme.css"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_444444_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_444444_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_555555_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_555555_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_777620_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_777620_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_777777_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_777777_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_cc0000_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_cc0000_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/themes/base/images/ui-icons_ffffff_256x240.png",
        mime: "image/png",
        inject: false,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/themes/base/images/ui-icons_ffffff_256x240.png"),
    },
    Asset {
        name: "jquery-ui-1.13.3/jquery-ui.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-1.13.3/jquery-ui.min.js"),
    },
    Asset {
        name: "jquery-ui-touch-punch-0.2.3.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/jquery-ui-touch-punch-0.2.3.min.js"),
    },
    Asset {
        name: "showdown-2.1.0.min.js",
        mime: "application/javascript; charset=utf-8",
        inject: true,
        bytes: include_bytes!("../../assets/card-libs/showdown-2.1.0.min.js"),
    },
];

/// 全部可服务条目名（含仅供 CSS 引用的字体/图片）。
pub fn names() -> Vec<&'static str> {
    ASSETS.iter().map(|a| a.name).collect()
}

/// 可注入条目的名字，**顺序即注入顺序**（与前端 `CARD_LIB_FILES` 必须同名同序）。
pub fn injectable() -> Vec<&'static str> {
    ASSETS.iter().filter(|a| a.inject).map(|a| a.name).collect()
}

/// GET /card-libs/<name>：整名精确命中则回资源，否则 404。
/// 不做路径拼接/解码 —— 名字必须是清单里的字面量（含子目录者亦然）。
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
    fn injectable_set_is_expected_and_ordered() {
        // 注入集与顺序（升级/换库必须显式改这里 + LICENSES.md + 前端清单）
        assert_eq!(
            injectable(),
            vec![
                "jquery-3.7.1.min.js",
                "toastr-2.1.4.min.css",
                "toastr-2.1.4.min.js",
                "lodash-4.17.21.min.js",
                "js-yaml-4.1.0.min.js",
                "vue-3.5.13.global.prod.js",
                "vue-router-4.5.0.global.prod.js",
                "fontawesome-6.7.2/css/all.min.css",
                "jquery-ui-1.13.3/themes/base/theme.css",
                "jquery-ui-1.13.3/jquery-ui.min.js",
                "jquery-ui-touch-punch-0.2.3.min.js",
                "showdown-2.1.0.min.js",
            ]
        );
    }

    #[test]
    fn servable_set_covers_accessory_assets() {
        // 附属资源（字体/图片）不可注入，但必须可服务 —— 它们由 CSS 相对引用
        let all = names();
        assert_eq!(all.len(), 22, "可服务条目应为 12 可注入 + 10 附属");
        for a in [
            "fontawesome-6.7.2/webfonts/fa-solid-900.woff2",
            "fontawesome-6.7.2/webfonts/fa-v4compatibility.woff2",
            "jquery-ui-1.13.3/themes/base/images/ui-icons_444444_256x240.png",
        ] {
            assert!(all.contains(&a), "{a} 应可服务");
            assert!(!injectable().contains(&a), "{a} 不应被注入");
        }
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
            assert!(
                ct.starts_with("application/javascript")
                    || ct.starts_with("text/css")
                    || ct.starts_with("font/woff2")
                    || ct.starts_with("image/png"),
                "{name} content-type 不符：{ct}"
            );
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
            // 子目录形态也必须整名命中：目录本身、缺文件名都不行
            "/card-libs/fontawesome-6.7.2/",
            "/card-libs/fontawesome-6.7.2/css/",
            "/card-libs/fontawesome-6.7.2/css/../css/all.min.css",
            "/card-libs/fontawesome-6.7.2/webfonts/fa-solid-900.ttf", // 未打的 ttf
            // 不做百分号解码：编码过的名字不匹配
            "/card-libs/%6aquery-3.7.1.min.js",
            "/card-libs/whatever.js",
            // 前缀之外（调用方不该走到这里，兜底仍 404）
            "/card-libs",
            "/characters/x.png",
        ] {
            assert_eq!(serve(p, None).status(), StatusCode::NOT_FOUND, "{p} 应 404");
        }
    }
}
