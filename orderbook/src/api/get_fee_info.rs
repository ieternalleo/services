use crate::fee::MinFeeCalculator;

use super::H160Wrapper;
use anyhow::Result;
use chrono::{DateTime, Utc};
use model::u256_decimal;
use primitive_types::{H160, U256};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use warp::{hyper::StatusCode, reply, Filter, Rejection, Reply};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeeInfo {
    pub expiration_date: DateTime<Utc>,
    #[serde(with = "u256_decimal")]
    pub minimal_fee: U256,
    pub fee_ratio: u32,
}

pub fn get_fee_info_request() -> impl Filter<Extract = (H160,), Error = Rejection> + Clone {
    warp::path!("tokens" / H160Wrapper / "fee")
        .and(warp::get())
        .map(|token: H160Wrapper| token.0)
}

pub fn get_fee_info_response(result: Result<Option<(U256, DateTime<Utc>)>>) -> impl Reply {
    match result {
        Ok(Some((minimal_fee, expiration_date))) => {
            let fee_info = FeeInfo {
                expiration_date,
                minimal_fee,
                fee_ratio: 0u32,
            };
            Ok(reply::with_status(reply::json(&fee_info), StatusCode::OK))
        }
        Ok(None) => Ok(reply::with_status(
            super::error("NotFound", "Token was not found"),
            StatusCode::NOT_FOUND,
        )),
        Err(err) => {
            tracing::error!(?err, "get_fee error");
            Ok(reply::with_status(
                super::internal_error(),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub fn get_fee_info(
    fee_calculator: Arc<MinFeeCalculator>,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    get_fee_info_request().and_then(move |token| {
        let fee_calculator = fee_calculator.clone();
        async move {
            Result::<_, Infallible>::Ok(get_fee_info_response(fee_calculator.min_fee(token).await))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::response_body;
    use chrono::FixedOffset;
    use warp::test::request;

    #[tokio::test]
    async fn get_fee_info_request_ok() {
        let filter = get_fee_info_request();
        let token = String::from("0x0000000000000000000000000000000000000001");
        let path_string = format!("/tokens/{}/fee", token);
        let request = request().path(&path_string).method("GET");
        let result = request.filter(&filter).await.unwrap();
        assert_eq!(result, H160::from_low_u64_be(1));
    }

    #[tokio::test]
    async fn get_fee_info_response_() {
        let response =
            get_fee_info_response(Ok(Some((U256::zero(), Utc::now() + FixedOffset::east(10)))))
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body(response).await;
        let body: FeeInfo = serde_json::from_slice(body.as_slice()).unwrap();
        assert_eq!(body.minimal_fee, U256::zero());
        assert_eq!(body.fee_ratio, 0);
        assert!(body.expiration_date.gt(&chrono::offset::Utc::now()))
    }
}
