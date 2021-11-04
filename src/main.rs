use std::collections::HashSet;

use clap::Parser;

use sites::{Site, Submission, SubmissionSite};

mod sites;

#[derive(clap::Parser)]
#[clap(version = env!("CARGO_PKG_VERSION"), author = env!("CARGO_PKG_AUTHORS"))]
struct Opts {
    /// Path to database file to store information about loaded submissions.
    #[clap(long, default_value = "submissions.db")]
    submissions_database: String,

    /// API key to access Weasyl submissions.
    #[clap(long)]
    weasyl_api_key: String,
    /// FurAffinity cookie 'a'.
    #[clap(long)]
    furaffinity_cookie_a: String,
    /// FurAffinity cookie 'b'.
    #[clap(long)]
    furaffinity_cookie_b: String,

    /// Weasyl username.
    #[clap(long)]
    weasyl_user: String,
    /// FurAffinity username.
    #[clap(long)]
    furaffinity_user: String,

    #[clap(subcommand)]
    command: Command,
}

/// A tool to add or remove tags from FurAffinity and Weasyl submissions based
/// on existing tags.
#[derive(clap::Parser)]
enum Command {
    /// Download all submissions from sites.
    LoadSubmissions,
    /// Locally query submissions based on tags.
    QueryTags {
        /// Tags to include in search results.
        #[clap(long)]
        search: String
    },
    /// Update submissions matching a given search to include new tags.
    ApplyTags {
        /// Only print out changes instead of applying them.
        #[clap(short, long)]
        dry_run: bool,
        /// Search for submissions with given tags to update.
        #[clap(long)]
        search: String,
        /// New tags to apply to matched submissions.
        #[clap(long)]
        tags: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let opts = Opts::parse();

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&format!("sqlite://{}?mode=rwc", opts.submissions_database))
        .await
        .unwrap();

    sqlx::migrate!().run(&pool).await.unwrap();

    let weasyl = sites::Weasyl::new(&opts.weasyl_api_key, opts.weasyl_user);
    let furaffinity = sites::FurAffinity::new(
        &opts.furaffinity_cookie_a,
        &opts.furaffinity_cookie_b,
        opts.furaffinity_user,
    );

    match opts.command {
        Command::LoadSubmissions => {
            let submissions = weasyl
                .get_all_submissions()
                .await?
                .into_iter()
                .chain(furaffinity.get_all_submissions().await?.into_iter());

            let mut tx = pool.begin().await?;
            sqlx::query!("DELETE FROM submission")
                .execute(&mut tx)
                .await?;

            for submission in submissions {
                let site = submission.site.as_str();
                let tags = serde_json::to_value(submission.tags)?;

                let posted_at = chrono::DateTime::<chrono::Utc>::from(submission.posted_at);

                sqlx::query!(
                    "INSERT OR IGNORE INTO submission (site, id, title, posted_at, tags) VALUES ($1, $2, $3, $4, $5)",
                    site, submission.id, submission.title, posted_at, tags
                ).execute(&mut tx).await?;
            }

            tx.commit().await?;
        }
        Command::QueryTags { search } => {
            let submissions = get_submissions(&pool).await?;
            let filtered_submissions = query_submissions(&submissions, &search);

            for sub in filtered_submissions {
                tracing::info!(
                    "{}-{} - {}, {}: {}",
                    sub.site,
                    sub.id,
                    sub.posted_at.format("%Y-%m-%d"),
                    sub.title,
                    sub.tags.join(", ")
                );
            }
        }
        Command::ApplyTags {
            dry_run,
            search,
            tags,
        } => {
            let submissions = get_submissions(&pool).await?;
            let filtered_submissions = query_submissions(&submissions, &search);

            if dry_run {
                for sub in filtered_submissions {
                    let _span =
                        tracing::info_span!("Dry run", id = sub.id, site = %sub.site).entered();

                    let new_tags = update_tags(&sub.tags, &tags);
                    tag_display(&sub.tags, &new_tags);
                }
            } else {
                for sub in filtered_submissions {
                    let _span = tracing::info_span!("Updating tags", id = sub.id, site = %sub.site)
                        .entered();

                    let new_tags = update_tags(&sub.tags, &tags);
                    tracing::info!("Setting tags to: {}", new_tags.join(", "));

                    match sub.site {
                        SubmissionSite::FurAffinity => {
                            furaffinity.set_tags(sub.id, &new_tags).await?
                        }
                        SubmissionSite::Weasyl => weasyl.set_tags(sub.id, &new_tags).await?,
                    }

                    let tag_value = serde_json::to_value(&new_tags)?;
                    let site = sub.site.as_str();
                    sqlx::query!(
                        "UPDATE submission SET tags = $1 WHERE site = $2 AND id = $3",
                        tag_value,
                        site,
                        sub.id
                    )
                    .execute(&pool)
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn get_submissions(pool: &sqlx::Pool<sqlx::Sqlite>) -> anyhow::Result<Vec<Submission>> {
    let submissions = sqlx::query!("SELECT site, id, title, posted_at, tags FROM submission")
        .map(|row| -> anyhow::Result<Submission> {
            let posted_at: chrono::DateTime<chrono::Local> =
                chrono::DateTime::<chrono::Utc>::from_utc(row.posted_at, chrono::Utc).into();

            let site = match row.site.as_ref() {
                "FurAffinity" => SubmissionSite::FurAffinity,
                "Weasyl" => SubmissionSite::Weasyl,
                _ => anyhow::bail!("unknown site in database"),
            };

            let tags: Vec<String> = serde_json::from_str(&row.tags)?;

            Ok(Submission {
                id: row.id as i32,
                site,
                title: row.title,
                posted_at,
                tags,
            })
        })
        .fetch_all(pool)
        .await?
        .into_iter()
        .filter_map(|row| row.ok())
        .collect();

    Ok(submissions)
}

fn query_submissions<'a>(submissions: &'a [Submission], query: &str) -> Vec<&'a Submission> {
    let query_tags: Vec<_> = query
        .split(' ')
        .map(|tag| tag.to_ascii_lowercase())
        .collect();
    let required_tags: Vec<_> = query_tags
        .iter()
        .filter(|tag| !tag.starts_with('-'))
        .collect();
    let skipped_tags: Vec<_> = query_tags
        .iter()
        .filter(|tag| tag.starts_with('-'))
        .map(|tag| tag.chars().skip(1).collect())
        .collect();

    submissions
        .iter()
        .filter(|sub| {
            let tags: Vec<_> = sub
                .tags
                .iter()
                .map(|tag| tag.to_ascii_lowercase())
                .collect();

            required_tags.iter().all(|tag| tags.contains(tag))
                && !skipped_tags.iter().any(|tag| tags.contains(tag))
        })
        .collect()
}

fn update_tags(tags: &[String], changes: &str) -> Vec<String> {
    let change_tags: Vec<_> = changes.split(' ').collect();
    let add_tags = change_tags.iter().filter(|tag| !tag.starts_with('-'));
    let remove_tags: Vec<_> = change_tags
        .iter()
        .filter(|tag| tag.starts_with('-'))
        .map(|tag| tag.chars().skip(1).collect::<String>())
        .map(|tag| tag.to_ascii_lowercase())
        .collect();

    let mut tags = tags.to_vec();
    tags.extend(add_tags.into_iter().map(|tag| tag.to_string()));
    tags.retain(|tag| !remove_tags.contains(&tag.to_ascii_lowercase()));

    tags
}

fn tag_display(old: &[String], new: &[String]) {
    let old: HashSet<&String> = HashSet::from_iter(old.iter());
    let new: HashSet<&String> = HashSet::from_iter(new.iter());

    let added = new.difference(&old);
    let removed = old.difference(&new);

    tracing::info!(
        "Adding tags: {}",
        added
            .into_iter()
            .map(|tag| tag.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    tracing::info!(
        "Removing tags: {}",
        removed
            .into_iter()
            .map(|tag| tag.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[cfg(test)]
mod tests {
    use crate::{
        query_submissions,
        sites::{Submission, SubmissionSite},
        update_tags,
    };

    #[test]
    fn test_query_submissions() {
        let submissions = vec![
            Submission {
                id: 1,
                site: SubmissionSite::FurAffinity,
                title: "test".to_string(),
                posted_at: chrono::Local::now(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
            },
            Submission {
                id: 2,
                site: SubmissionSite::FurAffinity,
                title: "test".to_string(),
                posted_at: chrono::Local::now(),
                tags: vec!["tag3".to_string()],
            },
            Submission {
                id: 3,
                site: SubmissionSite::FurAffinity,
                title: "test".to_string(),
                posted_at: chrono::Local::now(),
                tags: vec!["tag1".to_string(), "tag4".to_string()],
            },
        ];

        let items = query_submissions(&submissions, "tag1 -tag4");
        assert_eq!(items.iter().map(|sub| sub.id).collect::<Vec<_>>(), vec![1]);

        let items = query_submissions(&submissions, "tag1 tag2");
        assert_eq!(items.iter().map(|sub| sub.id).collect::<Vec<_>>(), vec![1]);

        let items = query_submissions(&submissions, "tag1");
        assert_eq!(
            items.iter().map(|sub| sub.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn test_update_tags() {
        let tags = vec!["tag1".to_string(), "tag2".to_string()];
        let new_tags = update_tags(&tags, "tag3 -tag2");
        assert_eq!(new_tags, vec!["tag1".to_string(), "tag3".to_string()]);
    }
}
