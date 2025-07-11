use deadpool_postgres::GenericClient;
use fedimint_core::config::{ClientConfig, ClientModuleConfig, JsonClientConfig, JsonWithKind};
use fedimint_core::core::{ModuleInstanceId, ModuleKind};
use fedimint_core::encoding::DynRawFallback;
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fedimint_core::module::CommonModuleInit;
use fedimint_core::util::SafeUrl;
use fedimint_ln_common::LightningCommonInit;
use fedimint_mint_common::MintCommonInit;
use fedimint_wallet_common::WalletCommonInit;
use hex::ToHex;
use postgres_from_row::FromRow;
use serde_json::json;
#[cfg(feature = "stability_pool")]
use stability_pool_common::StabilityPoolCommonGen;
#[cfg(feature = "stability_pool")]
use stability_pool_common_old::StabilityPoolCommonGen as StabilityPoolCommonGenOld;
use tracing::debug;

pub fn config_to_json(cfg: ClientConfig) -> anyhow::Result<JsonClientConfig> {
    let decoders = get_decoders(
        cfg.modules
            .iter()
            .map(|(module_instance_id, module_config)| {
                (*module_instance_id, module_config.kind.clone())
            }),
    );
    let config = cfg.redecode_raw(&decoders)?;

    Ok(JsonClientConfig {
        global: config.global,
        modules: config
            .modules
            .into_iter()
            .map(
                |(
                    instance_id,
                    ClientModuleConfig {
                        kind,
                        config: module_config,
                        ..
                    },
                )| {
                    (
                        instance_id,
                        JsonWithKind::new(
                            kind.clone(),
                            match module_config {
                                DynRawFallback::Raw { raw, .. } => {
                                    let raw: String = ToHex::encode_hex(&raw);
                                    json!({"raw": raw})
                                }
                                DynRawFallback::Decoded(decoded) => decoded.to_json().into(),
                            },
                        ),
                    )
                },
            )
            .collect(),
    })
}

pub fn get_decoders(
    modules: impl IntoIterator<Item = (ModuleInstanceId, ModuleKind)>,
) -> ModuleDecoderRegistry {
    ModuleDecoderRegistry::new(modules.into_iter().filter_map(
        |(module_instance_id, module_kind)| {
            let decoder = match module_kind.as_str() {
                "ln" => LightningCommonInit::decoder(),
                "wallet" => WalletCommonInit::decoder(),
                "mint" => MintCommonInit::decoder(),
                #[cfg(feature = "stability_pool")]
                "stability_pool" => StabilityPoolCommonGenOld::decoder(),
                #[cfg(feature = "stability_pool")]
                "multi_sig_stability_pool" => StabilityPoolCommonGen::decoder(),
                _ => {
                    return None;
                }
            };

            Some((module_instance_id, module_kind, decoder))
        },
    ))
    .with_fallback()
}

pub async fn execute(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<u64> {
    let num_rows = conn.execute(sql, params).await?;
    Ok(num_rows)
}

pub async fn query_one<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<T>
where
    T: FromRow,
{
    let result = conn.query_one(sql, params).await?;
    Ok(T::try_from_row(&result)?)
}

pub async fn query_value<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<T>
where
    for<'a> T: tokio_postgres::types::FromSql<'a>,
{
    let result = conn.query_one(sql, params).await?;
    Ok(result.try_get(0)?)
}

pub async fn query_opt<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<Option<T>>
where
    T: FromRow,
{
    let result = conn.query_opt(sql, params).await?;
    Ok(result.map(|row| T::try_from_row(&row)).transpose()?)
}

pub async fn query<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<Vec<T>>
where
    T: FromRow,
{
    let result = conn.query(sql, params).await?;
    Ok(result
        .iter()
        .map(T::try_from_row)
        .collect::<Result<_, _>>()?)
}

pub fn cleanup_peer_url(api_endpoint: &fedimint_core::util::SafeUrl) -> SafeUrl {
    let api_endpoint_str = api_endpoint.as_str();
    const API_REPLACEMENT_LIST: &[(&str, &str)] = &[
        (
            "wss://outlying-mouse-4ex5u4hthfuo44e6z7gb.wnext.app/ws/",
            "wss://api.alpha.wlc.f.8fa.in/",
        ),
        (
            "wss://third-alligator-vrj3e2jue57qllu7ktje.wnext.app/ws/",
            "wss://api.bravo.wlc.f.8fa.in/",
        ),
        (
            "wss://blank-orc-e6o4bhwtlrasdrmfpend.wnext.app/ws/",
            "wss://api.charlie.wlc.f.8fa.in/",
        ),
        (
            "wss://dependable-distribution-rc47wuqts5mdhq35v7x6.wnext.app/ws/",
            "wss://api.delta.wlc.f.8fa.in/",
        ),
    ];

    let final_url_str = API_REPLACEMENT_LIST
        .iter()
        .find_map(|(search_url, replacement_url)| {
            if *search_url == api_endpoint_str {
                debug!(
                    "Replacing API URL '{search_url}' with '{replacement_url}', quick-fix for fedimint/fedimint#5482"
                );
                Some(*replacement_url)
            } else {
                None
            }
        })
        .unwrap_or(api_endpoint_str);

    final_url_str
        .parse()
        .expect("URL should be valid as it is derived from known-good URLs")
}
