use lazy_static::lazy_static;

use std::future::Future;

use flutter_rust_bridge::StreamSink;
use lightning_invoice::Invoice;
use moksha_core::model::PaymentRequest;
use moksha_fedimint::FedimintWallet;
use moksha_wallet::localstore::LocalStore;

use moksha_wallet::config_path;

use moksha_wallet::client::Client;
use moksha_wallet::wallet::{Wallet, WalletBuilder};
use std::str::FromStr;
use std::sync::Mutex as StdMutex;
use tokio::runtime::{Builder, Runtime};
use tracing::info;

use crate::memory_localstore::MemoryLocalStore;

#[cfg(not(target_arch = "wasm32"))]
static RUNTIME: once_cell::sync::Lazy<StdMutex<Runtime>> = once_cell::sync::Lazy::new(|| {
    StdMutex::new(
        Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime"),
    )
});

#[cfg(target_arch = "wasm32")]
static RUNTIME: once_cell::sync::Lazy<StdMutex<Runtime>> = once_cell::sync::Lazy::new(|| {
    StdMutex::new(
        Builder::new_current_thread()
            .build()
            .expect("Failed to create runtime"),
    )
});

lazy_static! {
    static ref MEMORY_LOCAL_STORE: MemoryLocalStore = MemoryLocalStore::default();
}

macro_rules! lock_runtime {
    () => {
        match RUNTIME.lock() {
            Ok(lock) => lock,
            Err(err) => {
                let err: anyhow::Error =
                    anyhow::anyhow!("Failed to lock the runtime mutex: {}", err);
                tracing::error!("Failed to lock the runtime mutex: {}", err);
                return Err(err.into());
            }
        }
    };
}

pub fn init_cashu() -> anyhow::Result<String> {
    let rt = RUNTIME.lock().unwrap();

    #[cfg(not(target_arch = "wasm32"))]
    let db_path = rt.block_on(async {
        use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .init();

        let db_path = config_path::db_path();
        let new_localstore =
            moksha_wallet::sqlx_localstore::SqliteLocalStore::with_path(db_path.clone())
                .await
                .map_err(anyhow::Error::from)
                .unwrap();
        new_localstore.migrate().await;
        db_path
    });
    drop(rt);

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async {
        tracing_wasm::set_as_global_default_with_config(
            tracing_wasm::WASMLayerConfigBuilder::default()
                .set_console_config(tracing_wasm::ConsoleConfig::ReportWithConsoleColor)
                .set_max_level(tracing::Level::INFO)
                .build(),
        );
        tracing::info!("tracing::info");
    });

    #[cfg(target_arch = "wasm32")]
    let db_path = "".to_owned();

    Ok(db_path)
}

pub fn get_cashu_balance(sink: StreamSink<u64>) -> anyhow::Result<()> {
    tracing::info!("get cashu balance");
    block_on(async move {
        let wallet = create_local_wallet().await.unwrap();
        let balance = wallet.get_balance().await.unwrap();
        tracing::info!("get cashu balance {}", balance);
        sink.add(balance);
        sink.close();
    });
    Ok(())
}

async fn create_local_wallet() -> anyhow::Result<Wallet<impl Client, impl LocalStore>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let db_path = config_path::db_path();
        let client = moksha_wallet::reqwest_client::HttpClient::new();
        let localstore =
            moksha_wallet::sqlx_localstore::SqliteLocalStore::with_path(db_path).await?;
        let mint_url = url::Url::parse("http://127.0.0.1:3338").expect("invalid url"); // FIXME redundant

        Ok(WalletBuilder::default()
            .with_client(client)
            .with_localstore(localstore)
            .with_mint_url(mint_url)
            .build()
            .await?)
    }

    #[cfg(target_arch = "wasm32")]
    {
        let client = crate::wasm_client::WasmClient::new();
        let mint_url = url::Url::parse("http://127.0.0.1:3338").expect("invalid url"); // FIXME redundant
        Ok(WalletBuilder::default()
            .with_client(client)
            .with_localstore(MEMORY_LOCAL_STORE.clone())
            .with_mint_url(mint_url)
            .build()
            .await?)
    }
}

async fn sleep_until(duration_ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep_until(
        tokio::time::Instant::now() + tokio::time::Duration::from_millis(duration_ms),
    )
    .await;

    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::sleep(core::time::Duration::from_millis(duration_ms)).await;
}

// FIXME return mint-status
pub fn cashu_mint_tokens(sink: StreamSink<u64>, amount: u64, hash: String) -> anyhow::Result<()> {
    block_on(async move {
        let wallet = create_local_wallet().await.unwrap();
        for _ in 0..30 {
            sleep_until(1_000).await;

            let mint_result = wallet.mint_tokens(amount.into(), hash.clone()).await;

            match mint_result {
                Ok(value) => {
                    sink.add(value.total_amount());
                    sink.close();
                    return;
                }
                Err(moksha_wallet::error::MokshaWalletError::InvoiceNotPaidYet(_, _)) => {
                    continue;
                }
                Err(e) => {
                    //return Err(e); // FIXME return error
                    break;
                }
            }
        }
        // Err(moksha_wallet::error::MokshaWalletError::InvoiceNotPaidYet(
        //     amount,
        //     "Invoice not paid yet".to_string(),
        // ))
    });

    Ok(())
}

fn block_on<F: Future<Output = ()> + 'static>(future: F) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rt = RUNTIME.lock().unwrap(); // FIXME handle error
        rt.block_on(future);
        drop(rt);
    }

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
}

pub fn get_cashu_mint_payment_request(
    sink: StreamSink<FlutterPaymentRequest>,
    amount: u64,
) -> anyhow::Result<()> {
    info!("get_cashu_mint_payment_request");

    block_on(async move {
        let wallet = create_local_wallet().await.unwrap();
        sink.add(
            wallet
                .get_mint_payment_request(amount)
                .await
                .unwrap()
                .into(),
        );
        sink.close();
    });
    Ok(())
}

// FIXME turn into sync function
pub fn decode_invoice(invoice: String) -> anyhow::Result<FlutterInvoice> {
    let invoice = Invoice::from_str(&invoice).map_err(anyhow::Error::from)?;
    Ok(invoice.into())
}

#[derive(Debug, Clone)]
pub struct FlutterInvoice {
    pub pr: String,
    pub amount_sats: u64,
    pub expiry_time: u64,
}

impl From<Invoice> for FlutterInvoice {
    fn from(invoice: Invoice) -> Self {
        Self {
            pr: invoice.to_string(),
            amount_sats: match invoice.amount_milli_satoshis() {
                Some(amount) => amount / 1000,
                None => 0,
            },
            expiry_time: invoice.expiry_time().as_secs(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FedimintPaymentRequest {
    pub pr: String,
    pub operation_id: String,
}

#[derive(Debug, Clone)]
pub struct FlutterPaymentRequest {
    pub pr: String,
    pub hash: String,
}

impl From<PaymentRequest> for FlutterPaymentRequest {
    fn from(value: PaymentRequest) -> Self {
        Self {
            pr: value.pr,
            hash: value.hash,
        }
    }
}

pub fn cashu_pay_invoice(sink: StreamSink<bool>, invoice: String) -> anyhow::Result<()> {
    block_on(async move {
        let wallet = create_local_wallet().await.unwrap();
        sink.add(wallet.pay_invoice(invoice).await.unwrap().paid);
        sink.close();
    });

    Ok(())
}

fn cashu_receive_token(sink: StreamSink<u64>, token: String) -> anyhow::Result<()> {
    let deserialized_token = token.try_into().map_err(anyhow::Error::from)?;

    info!("deserialized_token: {:?}", deserialized_token);
    block_on(async move {
        let wallet = create_local_wallet().await.unwrap();
        let _ = wallet.receive_tokens(&deserialized_token).await;
        sink.add(deserialized_token.total_amount());
        sink.close();
    });

    Ok(())
}

fn fedimint_workdir() -> std::path::PathBuf {
    // FIXME
    #[cfg(not(target_arch = "wasm32"))]
    return config_path::config_dir().join("fedimint");

    #[cfg(target_arch = "wasm32")]
    return std::path::PathBuf::from("/fedimint");
}

pub fn join_federation(federation: String) -> anyhow::Result<()> {
    //let rt = lock_runtime!();
    let rt = RUNTIME.lock().unwrap();
    let workdir = fedimint_workdir();

    rt.block_on(async {
        FedimintWallet::connect(workdir, &federation)
            .await
            .map_err(anyhow::Error::from)
    })?;

    drop(rt);
    Ok(())
}

pub fn get_fedimint_payment_request(amount: u64) -> anyhow::Result<FedimintPaymentRequest> {
    let workdir = fedimint_workdir();
    let rt = lock_runtime!();

    let result = rt.block_on(async {
        let wallet = FedimintWallet::new(workdir)
            .await
            .map_err(anyhow::Error::from)?;

        wallet
            .get_mint_payment_request(amount)
            .await
            .map_err(anyhow::Error::from)
    })?;

    drop(rt);

    Ok(FedimintPaymentRequest {
        pr: result.1.to_string(),
        operation_id: result.0,
    })
}

pub fn fedimint_mint_tokens(amount: u64, operation_id: String) -> anyhow::Result<u64> {
    let workdir = fedimint_workdir();
    let rt = lock_runtime!();

    rt.block_on(async {
        let wallet = FedimintWallet::new(workdir)
            .await
            .map_err(anyhow::Error::from)?;

        wallet.mint(operation_id).await.map_err(anyhow::Error::from)
    })?;

    drop(rt);
    Ok(amount)
}

pub fn get_fedimint_balance(sink: StreamSink<u64>) -> anyhow::Result<()> {
    block_on(async move {
        // let workdir = fedimint_workdir();
        // if !FedimintWallet::is_initialized(&workdir) {
        //     return;
        // }

        // sink.add(
        //     FedimintWallet::new(workdir)
        //         .await
        //         .unwrap()
        //         .balance()
        //         .await
        //         .unwrap(), // FIXME handle errors
        // );
        // FIXME
        sink.add(0);
        sink.close();
    });

    Ok(())
}

pub fn fedimint_pay_invoice(invoice: String) -> anyhow::Result<bool> {
    let rt = lock_runtime!();
    let workdir = fedimint_workdir();

    let result = rt.block_on(async {
        if !FedimintWallet::is_initialized(&workdir) {
            return Ok(false);
        }

        FedimintWallet::new(workdir)
            .await
            .map_err(anyhow::Error::from)?
            .pay_ln_invoice(invoice)
            .await
            .map_err(anyhow::Error::from)
    })?;

    drop(rt);
    Ok(result)
}

pub fn receive_token(sink: StreamSink<u64>, token: String) -> anyhow::Result<()> {
    if token.starts_with("cashu") {
        cashu_receive_token(sink, token)?;
    } else {
        fedimint_receive_token(token)?;
    }
    Ok(())
}

fn fedimint_receive_token(token: String) -> anyhow::Result<u64> {
    let rt = lock_runtime!();
    let workdir = fedimint_workdir();

    let result = rt.block_on(async {
        if !FedimintWallet::is_initialized(&workdir) {
            return Ok(0);
        }

        FedimintWallet::new(workdir)
            .await
            .map_err(anyhow::Error::from)?
            .receive_token(token)
            .await
            .map_err(anyhow::Error::from)
    })?;

    drop(rt);
    Ok(result)
}

pub fn get_btcprice(sink: StreamSink<f64>) -> anyhow::Result<()> {
    block_on(async move {
        let price = moksha_wallet::btcprice::get_btcprice()
            .await
            .map_err(anyhow::Error::from);
        sink.add(price.unwrap());
        sink.close();
    });

    Ok(())
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use flutter_rust_bridge::{rust2dart::Rust2Dart, StreamSink};
    use moksha_wallet::config_path;

    use super::{get_cashu_balance, init_cashu};

    #[test]
    fn test_get_balance() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir().expect("Could not create tmp dir");
        let tmp_dir = tmp.path().to_str().expect("Could not create tmp dir");

        std::env::set_var(config_path::ENV_DB_PATH, format!("{}/wallet.db", tmp_dir));
        let _ = init_cashu()?;
        let sink = StreamSink::new(Rust2Dart::new(0));
        let result = get_cashu_balance(sink);
        assert!(result.is_ok());
        Ok(())
    }
}
