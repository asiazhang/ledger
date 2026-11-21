use utoipa::ToSchema;

#[derive(ToSchema)]
#[allow(dead_code)]
pub struct ErrorResponse {
    kind: String,
    message: String,
    code: Option<String>,
    params: Option<Vec<String>>,
}
