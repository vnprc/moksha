use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::post;
use axum::Router;
use axum::{routing::get, Json};
use cashurs_core::model::{
    CheckFeesRequest, CheckFeesResponse, Keysets, PaymentRequest, PostMeltRequest,
    PostMeltResponse, PostMintRequest, PostMintResponse, PostSplitRequest, PostSplitResponse,
};
use dotenvy::dotenv;
use error::CashuMintError;
use hyper::Method;
use mint::Mint;
use model::{GetMintQuery, PostMintQuery};
use secp256k1::PublicKey;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{debug, event, Level};

use crate::lightning::LnbitsLightning;
use std::env;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod database;
mod error;
mod lightning;
mod mint;
mod model;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();
    event!(Level::INFO, "startup");

    let addr = "[::]:3338".parse()?;
    event!(Level::INFO, "listening on {}", addr);

    dotenv().expect(".env file not found");
    let mint = create_mint();

    axum::Server::bind(&addr)
        .serve(
            app(mint)
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods([Method::GET, Method::POST]),
                )
                .into_make_service(),
        )
        .await?;

    Ok(())
}

fn create_mint() -> Mint {
    let ln = Arc::new(LnbitsLightning::new(
        env::var("LNBITS_WALLET_ID").expect("LNBITS_WALLET_ID not found"),
        env::var("LNBITS_ADMIN_KEY").expect("LNBITS_ADMIN_KEY not found"),
        env::var("LNBITS_INVOICE_READ_KEY").expect("LNBITS_INVOICE_READ_KEY not found"),
        env::var("LNBITS_URL").expect("LNBITS_URL not found"),
    ));

    Mint::new(
        env::var("MINT_PRIVATE_KEY").expect("MINT_PRIVATE_KEY not found"),
        ln,
        env::var("MINT_DB_PATH").expect("MINT_DB_PATH not found"),
    )
}

fn app(mint: Mint) -> Router {
    Router::new()
        .route("/keys", get(get_keys))
        .route("/keysets", get(get_keysets))
        .route("/mint", get(get_mint).post(post_mint))
        .route("/checkfees", post(post_check_fees))
        .route("/melt", post(post_melt))
        .route("/split", post(post_split))
        .with_state(mint)
        .layer(TraceLayer::new_for_http())
}

async fn post_split(
    State(mint): State<Mint>,
    Json(split_request): Json<PostSplitRequest>,
) -> Result<Json<PostSplitResponse>, CashuMintError> {
    let (fst, snd) = mint
        .split(
            split_request.amount,
            split_request.proofs,
            split_request.outputs,
        )
        .await?;

    Ok(Json(PostSplitResponse { fst, snd }))
}

async fn post_melt(
    State(mint): State<Mint>,
    Json(melt_request): Json<PostMeltRequest>,
) -> Result<Json<PostMeltResponse>, CashuMintError> {
    let (paid, preimage, change) = mint.melt(melt_request.pr, melt_request.proofs).await?;

    Ok(Json(PostMeltResponse {
        paid,
        preimage,
        change,
    }))
}

async fn post_check_fees(
    Json(_check_fees): Json<CheckFeesRequest>,
) -> Result<Json<CheckFeesResponse>, CashuMintError> {
    Ok(Json(CheckFeesResponse { fee: 0 }))
}

async fn get_mint(
    State(mint): State<Mint>,
    Query(mint_query): Query<GetMintQuery>,
) -> Result<Json<PaymentRequest>, CashuMintError> {
    debug!("amount: {mint_query:#?}");

    let (pr, hash) = mint.create_invoice(mint_query.amount).await?;
    Ok(Json(PaymentRequest { pr, hash }))
}

async fn post_mint(
    State(mint): State<Mint>,
    Query(mint_query): Query<PostMintQuery>,
    Json(blinded_messages): Json<PostMintRequest>,
) -> Result<Json<PostMintResponse>, CashuMintError> {
    event!(
        Level::INFO,
        "post_mint: {mint_query:#?} {blinded_messages:#?}"
    );

    let promises = mint
        .mint_tokens(mint_query.hash, blinded_messages.outputs)
        .await?;
    Ok(Json(PostMintResponse { promises }))
}

async fn get_keys(
    State(mint): State<Mint>,
) -> Result<Json<HashMap<u64, PublicKey>>, CashuMintError> {
    Ok(Json(mint.keyset.public_keys))
}

async fn get_keysets(State(mint): State<Mint>) -> Result<Json<Keysets>, CashuMintError> {
    Ok(Json(Keysets {
        keysets: vec![mint.keyset.keyset_id],
    }))
}
