use actix_web::web::Data;
use actix_web::HttpResponse;

use jsonwebtoken::{decode, DecodingKey, Validation};
use log::error;

use serde::{Deserialize, Serialize};

use crate::core::{AppStateFeedback, TokenRecord};

// Additionally, there is a short delay until a token can be used.
// Clients need to wait that time if (for some reason) the user submitted
// faster than limited here.
const TOKEN_MIN_AGE: usize = 5;
const TOKEN_MAX_AGE: usize = 3600 * 12; // 12h

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    exp: usize, // Required (validate_exp defaults to true in validation). Expiration time (as UTC timestamp)
    iat: usize, // Optional. Issued at (as UTC timestamp)
    nbf: usize, // Optional. Not Before (as UTC timestamp)
    kid: u64,   // Optional. Key ID
}

impl Claims {
    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp() as usize;
        Self {
            exp: now + TOKEN_MAX_AGE,
            iat: now,
            nbf: now + TOKEN_MIN_AGE,
            kid: rand::random(),
        }
    }
}

pub async fn validate_token(
    state: &Data<AppStateFeedback>,
    supplied_token: &str,
) -> Option<HttpResponse> {
    if !state.able_to_process_feedback() {
        return Some(
            HttpResponse::ServiceUnavailable()
                .content_type("text/plain")
                .body("Feedback is currently not configured on this server."),
        );
    }

    let secret = std::env::var("JWT_KEY").unwrap(); // we checked the ability to process feedback
    let x = DecodingKey::from_secret(secret.as_bytes());
    let jwt_token = decode::<Claims>(supplied_token, &x, &Validation::default());
    let kid = match jwt_token {
        Ok(token) => token.claims.kid,
        Err(e) => {
            error!("Failed to decode token: {:?}", e.kind());
            return Some(HttpResponse::Forbidden().content_type("text/plain").body(
                match e.kind() {
                    jsonwebtoken::errors::ErrorKind::ImmatureSignature => "Token is not yet valid.",
                    jsonwebtoken::errors::ErrorKind::ExpiredSignature => "Token expired",
                    _ => "Invalid token",
                },
            ));
        }
    };

    // now we know from token-validity, that it is within our time limits and created by us.
    // The problem is, that it could be used multiple times.
    // To prevent this, we need to check if the token was already used.
    // This is means that if this usage+our ratelimits are
    // - neither synced across multiple feedback instances, nor
    // - persisted between reboots

    let now = chrono::Utc::now().timestamp() as usize;
    let mut tokens = state.token_record.lock().await;
    // remove outdated tokens (no longer relevant for rate limit)
    tokens.retain(|t| t.next_reset > now);
    // check if token is already used
    if tokens.iter().any(|r| r.kid == kid) {
        return Some(
            HttpResponse::Forbidden()
                .content_type("text/plain")
                .body("Token already used."),
        );
    }
    tokens.push(TokenRecord {
        kid,
        next_reset: now + TOKEN_MAX_AGE,
    });
    None
}
