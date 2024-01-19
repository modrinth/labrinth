use crate::auth::AuthenticationError;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use std::fmt::{Debug, Display, Formatter};

pub struct Success<'a> {
    pub icon: &'a str,
    pub name: &'a str,
}

impl<'a> Success<'a> {
    pub fn render(self) -> Html<String> {
        let html = include_str!("success.html");

        Html(
            html.replace("{{ icon }}", self.icon)
                .replace("{{ name }}", self.name),
        )
    }
}

#[derive(Debug)]
pub struct ErrorPage {
    pub code: StatusCode,
    pub message: String,
}

impl Display for ErrorPage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let html = include_str!("error.html")
            .replace("{{ code }}", &self.code.to_string())
            .replace("{{ message }}", &self.message);
        write!(f, "{}", html)?;

        Ok(())
    }
}

impl IntoResponse for ErrorPage {
    fn into_response(self) -> Response {
        (self.code, Html(self.to_string())).into_response()
    }
}

impl From<AuthenticationError> for ErrorPage {
    fn from(item: AuthenticationError) -> Self {
        ErrorPage {
            code: item.status_code(),
            message: item.to_string(),
        }
    }
}
