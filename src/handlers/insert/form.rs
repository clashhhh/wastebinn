use crate::db::{write, Database};
use crate::handlers::extract::Theme;
use crate::handlers::html::make_error;
use crate::id::Id;
use crate::{Error, Page};
use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    pub extension: Option<String>,
    pub expires: Option<String>,
    pub password: String,
    pub title: String,
    #[serde(rename = "burn-after-reading")]
    pub burn_after_reading: Option<String>,
}

impl From<Entry> for write::Entry {
    fn from(entry: Entry) -> Self {
        let burn_after_reading = entry.burn_after_reading.map(|s| s == "on");
        let password = (!entry.password.is_empty()).then_some(entry.password);
        let title = (!entry.title.is_empty()).then_some(entry.title);
        let expires = entry
            .expires
            .and_then(|expires| expires.parse::<NonZeroU32>().ok());

        Self {
            text: entry.text,
            extension: entry.extension,
            expires,
            burn_after_reading,
            uid: None,
            password,
            title,
        }
    }
}

pub async fn post(
    State(page): State<Page>,
    State(db): State<Database>,
    jar: SignedCookieJar,
    headers: HeaderMap,
    theme: Option<Theme>,
    Form(entry): Form<Entry>,
) -> Result<(SignedCookieJar, Redirect), impl IntoResponse> {
    // TODO: think about something more appropriate because those headers might be all messed up
    // and yet we still have a proper TLS connection.
    let is_https = headers
        .get(http::header::HOST)
        .zip(headers.get(http::header::ORIGIN))
        .and_then(|(host, origin)| host.to_str().ok().zip(origin.to_str().ok()))
        .and_then(|(host, origin)| {
            origin
                .strip_prefix("https://")
                .map(|origin| origin.starts_with(host))
        })
        .unwrap_or(false);

    async {
        let id: Id = tokio::task::spawn_blocking(|| {
            let mut rng = rand::thread_rng();
            rng.gen::<u32>()
        })
        .await
        .map_err(Error::from)?
        .into();

        // Retrieve uid from cookie or generate a new one.
        let uid = if let Some(cookie) = jar.get("uid") {
            cookie
                .value()
                .parse::<i64>()
                .map_err(|err| Error::CookieParsing(err.to_string()))?
        } else {
            db.next_uid().await?
        };

        let mut entry: write::Entry = entry.into();
        entry.uid = Some(uid);

        let mut url = id.to_url_path(&entry);

        if entry.burn_after_reading.unwrap_or(false) {
            url = format!("burn/{url}");
        }

        db.insert(id, entry).await?;
        let url = format!("/{url}");

        let cookie = Cookie::build(("uid", uid.to_string()))
            .http_only(true)
            .secure(is_https)
            .same_site(SameSite::Strict)
            .build();

        Ok((jar.add(cookie), Redirect::to(&url)))
    }
    .await
    .map_err(|err| make_error(err, page, theme))
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::Client;
    use reqwest::{header, StatusCode};
    use std::collections::HashMap;

    #[tokio::test]
    async fn insert() -> Result<(), Box<dyn std::error::Error>> {
        let client = Client::new().await;

        let data = super::Entry {
            text: "FooBarBaz".to_string(),
            extension: Some("rs".to_string()),
            expires: None,
            password: "".to_string(),
            title: "".to_string(),
            burn_after_reading: None,
        };

        let res = client.post("/").form(&data).send().await?;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);

        let location = res.headers().get("location").unwrap().to_str()?;

        let res = client
            .get(location)
            .header(header::ACCEPT, "text/html; charset=utf-8")
            .send()
            .await?;

        assert_eq!(res.status(), StatusCode::OK);

        let header = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(header.to_str().unwrap().contains("text/html"));

        let content = res.text().await?;
        assert!(content.contains("FooBarBaz"));

        let res = client
            .get(&format!("/raw{location}"))
            .header(header::ACCEPT, "text/html; charset=utf-8")
            .send()
            .await?;

        assert_eq!(res.status(), StatusCode::OK);

        let header = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(header.to_str().unwrap().contains("text/plain"));

        let content = res.text().await?;
        assert_eq!(content, "FooBarBaz");

        Ok(())
    }

    #[tokio::test]
    async fn insert_fail() -> Result<(), Box<dyn std::error::Error>> {
        let client = Client::new().await;

        let mut data = HashMap::new();
        data.insert("Hello", "World");

        let res = client.post("/").form(&data).send().await?;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

        Ok(())
    }
}
