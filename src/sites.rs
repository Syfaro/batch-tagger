use std::fmt::Display;

use async_trait::async_trait;

pub use furaffinity::FurAffinity;
pub use weasyl::Weasyl;

#[derive(Debug)]
pub enum SubmissionSite {
    FurAffinity,
    Weasyl,
}

impl Display for SubmissionSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmissionSite::FurAffinity => {
                write!(f, "FurAffinity")
            }
            SubmissionSite::Weasyl => {
                write!(f, "Weasyl")
            }
        }
    }
}

impl SubmissionSite {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FurAffinity => "FurAffinity",
            Self::Weasyl => "Weasyl",
        }
    }
}

#[derive(Debug)]
pub struct Submission {
    pub id: i32,
    pub site: SubmissionSite,
    pub title: String,
    pub posted_at: chrono::DateTime<chrono::Local>,
    pub tags: Vec<String>,
}

#[async_trait]
pub trait Site {
    async fn get_all_submissions(&self) -> anyhow::Result<Vec<Submission>>;
    async fn set_tags(&self, id: i32, tags: &[String]) -> anyhow::Result<()>;
}

mod furaffinity {
    use std::collections::HashMap;

    use anyhow::Context;
    use async_trait::async_trait;
    use chrono::TimeZone;

    use super::*;

    pub struct FurAffinity {
        client: reqwest::Client,
        cookies: String,

        user: String,

        id_selector: scraper::Selector,
        title_selector: scraper::Selector,
        posted_at_selector: scraper::Selector,
        tag_selector: scraper::Selector,

        date_cleaner: regex::Regex,
    }

    #[derive(Debug)]
    struct EditData {
        key: String,
        cat: String,
        atype: String,
        species: String,
        gender: String,
        rating: String,
        title: String,
        message: String,
    }

    impl FurAffinity {
        pub fn new(cookie_a: &str, cookie_b: &str, user: String) -> Self {
            let mut cookies = HashMap::with_capacity(2);
            cookies.insert("a".to_string(), cookie_a.to_string());
            cookies.insert("b".to_string(), cookie_b.to_string());

            let client = reqwest::Client::default();

            let id_selector = scraper::Selector::parse(".submission-list u a").unwrap();
            let title_selector = scraper::Selector::parse(".submission-title h2 p").unwrap();
            let posted_at_selector =
                scraper::Selector::parse(".submission-id-sub-container strong span.popup_date")
                    .unwrap();
            let tag_selector = scraper::Selector::parse("section.tags-row a").unwrap();

            let date_cleaner = regex::Regex::new(r"(\d{1,2})(st|nd|rd|th)").unwrap();

            Self {
                client,
                cookies: Self::cookies(cookies),

                user,

                id_selector,
                title_selector,
                posted_at_selector,
                tag_selector,

                date_cleaner,
            }
        }

        fn cookies(cookies: HashMap<String, String>) -> String {
            cookies
                .into_iter()
                .map(|(name, value)| Self::cookie_string(&name, &value))
                .collect::<Vec<_>>()
                .join(";")
        }

        fn cookie_string(name: &str, value: &str) -> String {
            format!("{}={}", name, value)
        }

        fn join_text_nodes(elem: scraper::ElementRef) -> String {
            elem.text().collect::<Vec<_>>().join("").trim().to_string()
        }

        fn parse_document(page: &str) -> anyhow::Result<EditData> {
            use scraper::Selector;

            let html = scraper::Html::parse_document(page);

            let form = html
                .select(&Selector::parse(r#"form[name="MsgForm"]"#).unwrap())
                .next()
                .context("Page was missing form")?;

            let key = form
                .select(&Selector::parse(r#"input[name="key"]"#).unwrap())
                .next()
                .context("Form was missing key element")?
                .value()
                .attr("value")
                .context("Form was missing key value")?
                .to_string();

            let rating = form
                .select(&Selector::parse(r#"input[name="rating"][checked]"#).unwrap())
                .next()
                .context("Form was missing selected rating")?
                .value()
                .attr("value")
                .context("Form was missing selected rating value")?
                .to_string();

            let title = form
                .select(&Selector::parse("#title").unwrap())
                .next()
                .context("Form was missing title")?
                .value()
                .attr("value")
                .context("Form was missing title value")?
                .to_string();

            let message: String = form
                .select(&Selector::parse("#JSMessage").unwrap())
                .next()
                .context("Form was missing description")?
                .text()
                .collect();

            let cat = form
                .select(&Selector::parse(r#"select[name="cat"] option[selected]"#).unwrap())
                .next()
                .context("Form was missing selected category")?
                .value()
                .attr("value")
                .context("Form was missing category value")?
                .to_string();

            let atype = form
                .select(&Selector::parse(r#"select[name="atype"] option[selected]"#).unwrap())
                .next()
                .context("Form was missing selected atype")?
                .value()
                .attr("value")
                .context("Form was missing selected atype value")?
                .to_string();

            let species = form
                .select(&Selector::parse(r#"select[name="species"] option[selected]"#).unwrap())
                .next()
                .context("Form was missing selected species")?
                .value()
                .attr("value")
                .context("Form was missing selected species value")?
                .to_string();

            let gender = form
                .select(&Selector::parse(r#"select[name="gender"] option[selected]"#).unwrap())
                .next()
                .context("Form was missing selected gender")?
                .value()
                .attr("value")
                .context("Form was missing selected gender value")?
                .to_string();

            Ok(EditData {
                key,
                rating,
                title,
                message,
                cat,
                atype,
                species,
                gender,
            })
        }
    }

    #[async_trait]
    impl Site for FurAffinity {
        async fn get_all_submissions(&self) -> anyhow::Result<Vec<Submission>> {
            let mut ids = Vec::new();

            let mut page = 1;
            loop {
                tracing::info!(page, "Loading gallery page");

                let body = self
                    .client
                    .get(format!(
                        "https://www.furaffinity.net/gallery/{}/{}/",
                        self.user, page
                    ))
                    .header(reqwest::header::COOKIE, &self.cookies)
                    .send()
                    .await?
                    .text()
                    .await?;

                let body = scraper::Html::parse_document(&body);

                let mut new_ids = body
                    .select(&self.id_selector)
                    .into_iter()
                    .filter_map(|element| element.value().attr("href"))
                    .filter_map(|href| href.split('/').nth(2))
                    .filter_map(|id| id.parse::<i32>().ok())
                    .peekable();

                if new_ids.peek().is_none() {
                    tracing::debug!("No new IDs found");

                    break;
                }

                ids.extend(new_ids);
                page += 1;
            }

            tracing::info!("Discovered {} submissions", ids.len());

            let mut submissions = Vec::with_capacity(ids.len());

            for id in ids {
                tracing::info!(id, "Loading complete information for submission");

                let submission = self
                    .client
                    .get(format!("https://www.furaffinity.net/view/{}/", id))
                    .header(reqwest::header::COOKIE, &self.cookies)
                    .send()
                    .await?
                    .text()
                    .await?;

                let body = scraper::Html::parse_document(&submission);

                let title = Self::join_text_nodes(
                    body.select(&self.title_selector)
                        .next()
                        .context("Submission must have title")?,
                );

                let posted_at = body
                    .select(&self.posted_at_selector)
                    .next()
                    .context("Missing posted at date")?
                    .value()
                    .attr("title")
                    .context("Missing posted at value")?;
                let posted_at = self.date_cleaner.replace(posted_at, "$1");
                let posted_at = chrono::Local
                    .datetime_from_str(&posted_at, "%b %e, %Y %l:%M %p")
                    .context("Unknown date format")?;

                let tags: Vec<String> = body
                    .select(&self.tag_selector)
                    .into_iter()
                    .map(Self::join_text_nodes)
                    .collect();

                submissions.push(Submission {
                    site: SubmissionSite::FurAffinity,
                    id,
                    title,
                    posted_at,
                    tags,
                });
            }

            Ok(submissions)
        }

        async fn set_tags(&self, id: i32, tags: &[String]) -> anyhow::Result<()> {
            let url = format!(
                "https://www.furaffinity.net/controls/submissions/changeinfo/{}/",
                id
            );

            let page = self
                .client
                .get(&url)
                .header(reqwest::header::COOKIE, &self.cookies)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;

            let data = Self::parse_document(&page)?;

            let body = [
                ("update", "yes".to_string()),
                ("submit", "+Finalize".to_string()),
                ("keywords", tags.join(" ")),
                ("key", data.key),
                ("cat", data.cat),
                ("atype", data.atype),
                ("species", data.species),
                ("gender", data.gender),
                ("rating", data.rating),
                ("title", data.title),
                ("message", data.message),
            ];

            self.client
                .post(url)
                .header(reqwest::header::COOKIE, &self.cookies)
                .form(&body)
                .send()
                .await?
                .error_for_status()?;

            Ok(())
        }
    }
}

mod weasyl {
    use std::collections::HashMap;

    use anyhow::Context;
    use async_trait::async_trait;
    use chrono::DateTime;
    use reqwest::header::{HeaderMap, HeaderValue};

    use super::*;

    pub struct Weasyl {
        client: reqwest::Client,
        user: String,
    }

    impl Weasyl {
        pub fn new(api_key: &str, user: String) -> Self {
            let mut headers: HeaderMap<HeaderValue> = reqwest::header::HeaderMap::with_capacity(1);
            headers.insert("X-Weasyl-API-Key", HeaderValue::from_str(api_key).unwrap());

            let client = reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap();

            Self { client, user }
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct WeasylSubmission {
        submitid: i32,
        title: String,
        #[serde(deserialize_with = "datetime_from_weasyl_str")]
        posted_at: chrono::DateTime<chrono::Utc>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct WeasylSubmissionFull {
        submitid: i32,
        title: String,
        tags: Vec<String>,
    }

    fn datetime_from_weasyl_str<'de, D>(
        deserializer: D,
    ) -> Result<chrono::DateTime<chrono::Utc>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(DateTime::<chrono::Utc>::from)
            .map_err(serde::de::Error::custom)
    }

    #[derive(Debug, serde::Deserialize)]
    struct WeasylSubmissionResponse {
        backid: Option<i32>,
        nextid: Option<i32>,

        submissions: Vec<WeasylSubmission>,
    }

    #[async_trait]
    impl Site for Weasyl {
        async fn get_all_submissions(&self) -> anyhow::Result<Vec<Submission>> {
            let mut submissions = Vec::new();

            let mut nextid: Option<i32> = None;

            loop {
                tracing::info!(?nextid, "Loading submission page");

                let mut params = HashMap::with_capacity(1);
                params.insert("count", "100".to_string());
                if let Some(nextid) = nextid {
                    params.insert("nextid", nextid.to_string());
                }

                let page: WeasylSubmissionResponse = self
                    .client
                    .get(format!(
                        "https://www.weasyl.com/api/users/{}/gallery",
                        self.user
                    ))
                    .query(&params)
                    .send()
                    .await
                    .context("Could not make request for gallery")?
                    .error_for_status()
                    .context("Got bad gallery status code")?
                    .json()
                    .await
                    .context("Could not decode gallery")?;

                submissions.extend(page.submissions);

                if let Some(id) = page.nextid {
                    nextid = Some(id);
                } else {
                    break;
                }
            }

            tracing::info!("Discovered {} submissions", submissions.len());

            let mut completed_submissions = Vec::with_capacity(submissions.len());

            for sub in submissions {
                tracing::info!(
                    id = sub.submitid,
                    "Loading complete information for submission"
                );

                let submission: WeasylSubmissionFull = self
                    .client
                    .get(format!(
                        "https://www.weasyl.com/api/submissions/{}/view",
                        sub.submitid
                    ))
                    .send()
                    .await
                    .context("Could not make request for submission")?
                    .error_for_status()
                    .context("Got bad submission status code")?
                    .json()
                    .await
                    .context("Could not decode submission")?;

                completed_submissions.push(Submission {
                    site: SubmissionSite::Weasyl,
                    id: submission.submitid,
                    title: submission.title,
                    posted_at: sub.posted_at.into(),
                    tags: submission.tags,
                });
            }

            Ok(completed_submissions)
        }

        async fn set_tags(&self, id: i32, tags: &[String]) -> anyhow::Result<()> {
            let tags = tags.join(" ");

            self.client
                .post("https://www.weasyl.com/submit/tags")
                .form(&[("submitid", id.to_string()), ("tags", tags)])
                .send()
                .await?
                .error_for_status()?;

            Ok(())
        }
    }
}
