/*
 * [INPUT]: 依赖嵌入权限文案与配置/目标应用语言。
 * [OUTPUT]: 选择同屏文案与排版方向；续接文案指向 incodex perm。
 * [POS]: 权限卡的语言适配；不读取账号或权限状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

pub(super) fn native_guide_config(
    copy_context: GuideCopyContext,
    app_locale: Option<&str>,
) -> Result<NativeGuideConfig, String> {
    let source = incodex_runtime_assets::external_files()
        .iter()
        .find_map(|(name, body)| (*name == PERMISSION_COPY_NAME).then_some(*body))
        .ok_or("embedded native Accessibility copy catalog is missing")?;
    let mut catalog: Value = serde_json::from_str(source)
        .map_err(|error| format!("invalid native Accessibility copy catalog: {error}"))?;
    let catalog_object = catalog
        .as_object_mut()
        .ok_or("native Accessibility copy catalog is not an object")?;
    let raw_locale = configured_locale(app_locale);
    let locale = resolve_catalog_locale(&raw_locale, catalog_object);
    let mut copy = catalog_object
        .remove(&locale)
        .ok_or_else(|| format!("native Accessibility copy catalog has no locale: {locale}"))?;
    let copy_object = copy
        .as_object_mut()
        .ok_or("native Accessibility locale copy is not an object")?;
    choose_guide_copy(copy_object, copy_context)?;
    if let Some(error_body) = copy_object.get("errorBody").and_then(Value::as_str) {
        copy_object.insert(
            "errorBody".into(),
            Value::String(reentry_error_body(error_body)),
        );
    }
    let layout_direction = if is_rtl_locale(&locale) {
        "rightToLeft"
    } else {
        "leftToRight"
    };
    Ok(NativeGuideConfig {
        copy,
        layout_direction,
    })
}

pub(super) fn choose_guide_copy(
    copy: &mut serde_json::Map<String, Value>,
    context: GuideCopyContext,
) -> Result<(), String> {
    let (title_key, body_key) = match context {
        GuideCopyContext::Installed => ("installedTitle", "body"),
        GuideCopyContext::Official => ("title", "officialBody"),
    };
    let title = copy
        .get(title_key)
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| format!("native Accessibility copy is missing {title_key}"))?
        .to_owned();
    let body = copy
        .get(body_key)
        .and_then(Value::as_str)
        .filter(|body| !body.trim().is_empty())
        .ok_or_else(|| format!("native Accessibility copy is missing {body_key}"))?
        .to_owned();
    copy.insert("title".into(), Value::String(title));
    copy.insert("body".into(), Value::String(body));
    Ok(())
}

pub(super) fn reentry_error_body(body: &str) -> String {
    body.replace("incodex install", "incodex perm")
        .replace("incodex accessibility", "incodex perm")
}

pub(super) fn configured_locale(app_locale: Option<&str>) -> String {
    let source_home = locale_source_home();
    let config =
        fs::read_to_string(source_home.join(incodex_core::session_layout::CONFIG_SETTING_FILE))
            .ok();
    let config_override = config
        .as_deref()
        .and_then(|content| crate::locale::parse_locale_override(content, &['"']));
    select_locale_source(config_override.as_deref(), app_locale)
}

pub(super) fn locale_source_home() -> PathBuf {
    let fallback = crate::open::default_source_home();
    resolve_locale_source_home(env::var_os("INCODEX_SOURCE_HOME").as_deref(), &fallback)
}

pub(super) fn resolve_locale_source_home(
    value: Option<&std::ffi::OsStr>,
    fallback: &Path,
) -> PathBuf {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return fallback.to_path_buf();
    };
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

pub(super) fn select_locale_source(
    config_override: Option<&str>,
    app_locale: Option<&str>,
) -> String {
    config_override
        .and_then(|value| (!value.trim().is_empty()).then_some(value.trim()))
        .or_else(|| app_locale.and_then(|value| (!value.trim().is_empty()).then_some(value.trim())))
        .unwrap_or("en")
        .into()
}

pub(super) fn resolve_catalog_locale(
    raw: &str,
    catalog: &serde_json::Map<String, Value>,
) -> String {
    let normalized = raw.trim().replace('_', "-");
    let normalized = normalized.split('.').next().unwrap_or("en");
    if catalog.contains_key(normalized) {
        return normalized.to_string();
    }
    let lower = normalized.to_ascii_lowercase();
    if let Some(locale) = catalog
        .keys()
        .find(|locale| locale.to_ascii_lowercase() == lower)
    {
        return locale.clone();
    }
    if lower.starts_with("zh-hant-hk") || lower.starts_with("zh-hk") {
        return "zh-HK".into();
    }
    if lower.starts_with("zh-hant") || lower.starts_with("zh-tw") {
        return "zh-TW".into();
    }
    if lower.starts_with("zh") {
        return "zh-CN".into();
    }
    if lower == "en" || lower.starts_with("en-") {
        return "en".into();
    }
    let language = lower.split('-').next().unwrap_or("en");
    if catalog.contains_key(language) {
        return language.into();
    }
    let override_locale = match language {
        "es" => Some("es-419"),
        "fr" => Some("fr-FR"),
        "no" => Some("nb-NO"),
        "pt" => Some("pt-BR"),
        _ => None,
    };
    if let Some(locale) = override_locale.filter(|locale| catalog.contains_key(*locale)) {
        return locale.into();
    }
    catalog
        .keys()
        .find(|locale| {
            locale
                .to_ascii_lowercase()
                .starts_with(&format!("{language}-"))
        })
        .cloned()
        .unwrap_or_else(|| "en".into())
}

pub(super) fn is_rtl_locale(locale: &str) -> bool {
    matches!(
        locale.to_ascii_lowercase().split('-').next(),
        Some("ar" | "fa" | "ur")
    )
}
