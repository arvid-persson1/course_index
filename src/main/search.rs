use actix_web::{get, HttpResponse, Responder, web};
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{de::Error as DeError, Deserialize, Deserializer};
use sqlx::QueryBuilder;

use courselib::{Category, Course, Difficulty, Language, Pace, Site};

use super::CONNECTION;

const ITEMS_PER_PAGE: u32 = 20;

lazy_static! {
    static ref CODE_PAT_VALIDATE: Regex = Regex::new(r"^[A-Z\d_]{6}$").expect("failed to parse regex");
    static ref SPECIAL_CHARACTERS: Regex = Regex::new(r"[^\pL\d\s]").expect("failed to parse regex");
}

#[derive(Deserialize, Debug, Clone)]
struct SearchQuery {
    #[serde(default)]
    page: u32,
    #[serde(default)]
    code_pattern: Option<String>,
    #[serde(default)]
    name_pattern: Option<String>,
    #[serde(default)]
    points: Option<f32>,
    #[serde(default)]
    paces: Option<Vec<Pace>>,
    // TODO: modules?
    #[serde(default)]
    period: Option<u8>,
    #[serde(default)]
    period_select_mode: PeriodSelectMode,
    #[serde(default)]
    sites: Vec<Site>,
    #[serde(default)]
    languages: Vec<Language>,
    #[serde(default)]
    difficulties: Vec<Difficulty>,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    category_select_mode: CategorySelectMode,
}

#[derive(Debug, Clone)]
enum PeriodSelectMode {
    Only,
    Starts,
    Ends,
    Spans,
}

impl<'de> Deserialize<'de> for PeriodSelectMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "only" => Ok(Self::Only),
            "starts" => Ok(Self::Starts),
            "ends" => Ok(Self::Ends),
            "spans" => Ok(Self::Spans),
            other => Err(DeError::unknown_variant(other, &["only", "starts", "ends", "spans"]))
        }
    }
}

impl Default for PeriodSelectMode {
    fn default() -> Self {
        Self::Only
    }
}

#[derive(Debug, Clone)]
enum CategorySelectMode {
    Any,
    All,
}

impl<'de> Deserialize<'de> for CategorySelectMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "any" => Ok(Self::Any),
            "all" => Ok(Self::All),
            other => Err(DeError::unknown_variant(other, &["any", "all"]))
        }
    }
}

impl Default for CategorySelectMode {
    fn default() -> Self {
        Self::Any
    }
}

async unsafe fn format_conditions(query: SearchQuery) -> Result<String, ()> {
    // Using string concatenation instead of binding values makes this code much more manageable.
    // STRING INPUTS MUST BE SANITIZED MANUALLY!

    let SearchQuery {
        page: _,
        code_pattern,
        mut name_pattern,
        points,
        paces,
        period,
        period_select_mode,
        sites,
        languages,
        difficulties,
        categories,
        category_select_mode,
    } = query;

    if code_pattern.as_deref().map(|p| !CODE_PAT_VALIDATE.is_match(p)).unwrap_or_default() {
        return Err(())
    }

    name_pattern = name_pattern.map(|p| SPECIAL_CHARACTERS.replace_all(&p, "").into());

    let conditions = vec![
        code_pattern.map(|p| format!("code ILIKE {}", p)),
        points.map(|p| format!("points = {}", p)),
        paces.map(|p|
            format!("pace in ({})", p
                .into_iter()
                .map(|s| format!("'{}'", s))
                .join(",")
            )),
        period.map(|p| match period_select_mode {
            PeriodSelectMode::Only => format!("period_start = {} AND period_end = {}", p, p),
            PeriodSelectMode::Starts => format!("period_start = {}", p),
            PeriodSelectMode::Ends => format!("period_end = {}", p),
            PeriodSelectMode::Spans => format!(
                "CASE
                WHEN period_start < period_end THEN ({} >= period_start AND {} <= period_end)
                ELSE ({} >= period_start OR {} <= period_end)
                END",
                p, p, p, p
            )
        }),
        Some(sites)
            .filter(|v| !v.is_empty())
            .map(|v| format!("site in ({})", v
                .into_iter()
                .map(|s| format!("'{}'", s))
                .join(",")
            )),
        Some(languages)
            .filter(|v| !v.is_empty())
            .map(|v| format!("language in ({})", v
                .into_iter()
                .map(|s| format!("'{}'", s))
                .join(",")
            )),
        Some(difficulties)
            .filter(|v| !v.is_empty())
            .map(|v| format!("difficulty in ({})", v
                .into_iter()
                .map(|s| format!("'{}'", s))
                .join(",")
            )),
        Some(categories)
            .filter(|v| !v.is_empty())
            .map(|v| format!("ARRAY[{}]::category_enum[] {} categories",
                v
                    .into_iter()
                    .map(|s| format!("'{}'", s))
                    .join(","),
                match category_select_mode {
                    CategorySelectMode::Any => "&&",
                    CategorySelectMode::All => "<@"
                }
            )),
        // This condition is moved to the end since it is by far the slowest and should therefor not run as often.
        name_pattern.map(|p| format!(r"REGEXP_REPLACE(name_se, '[^\pL\d\s]', '', 'g') ILIKE %{}% OR REGEXP_REPLACE(name_en, '[^\pL\d\s]', '', 'g') ILIKE %{}%", p, p)),
    ];

    if conditions.is_empty() {
        Ok("".into())
    } else {
        Ok(format!("WHERE {}", conditions
            .into_iter()
            .flatten()
            .join(" AND ")))
    }
}

#[get("/")]
pub async fn page(query: web::Query<SearchQuery>) -> impl Responder {
    let page_number = query.0.page;

    let conditions = if let Ok(conditions) = unsafe { format_conditions(query.0) }.await {
        conditions
    } else {
        // invalid query
        return HttpResponse::NotFound().finish() // FIXME
    };

    let res = QueryBuilder::new(format!(
        // TODO: what columns are needed?
        r#"SELECT code, name_se, name_en, url, points, pace, prerequisites, register_info, modules, period_start, period_end, site as "site: _", language as "language: _", difficulty as "difficulty: _", categories as "categories: _", conduct
        FROM courses
        {}
        ORDER BY id
        OFFSET {}
        LIMIT {}"#,
        conditions,
        page_number * ITEMS_PER_PAGE,
        ITEMS_PER_PAGE
    ))
        // TODO: remove type annotation
        .build_query_as::<Course>()
        .fetch_all(CONNECTION.get().unwrap())
        .await;

    match res {
        Ok(courses) => {
            /*let courses = stream::iter(courses)
                .filter_map(|c| async move {
                    /*let SearchQuery {
                        page,
                        code_pattern,
                        name_pattern,
                        points,
                        paces,
                        period,
                        period_select_mode,
                        sites,
                        language,
                        difficulties,
                        categories,
                        category_select_mode,
                    } = (&query.0).clone();*/

                    let Course {
                        code,
                        name_se,
                        name_en,
                        points,
                        pace,
                        period_start,
                        period_end,
                        site,
                        language,
                        difficulty,
                        categories,
                        ..
                    } = c;

                    Some(OptionFuture::from((&query).code_pattern
                        .as_ref()
                        .map(|pat| match_code_pattern(pat, code)))
                        .await
                        .unwrap_or(true)
                    && OptionFuture::from((&query).name_pattern
                        .as_ref()
                        .map(|pat| match_name_pattern(pat, name_se, name_en)))
                        .await
                        .unwrap_or(true))
                })
                .skip(query.page as usize * ITEMS_PER_PAGE)
                .take(ITEMS_PER_PAGE);*/

            HttpResponse::Ok()
                .body(todo!())
        }
        Err(e) => {
            eprintln!("{}", e);
            HttpResponse::InternalServerError().finish() // FIXME
        }
    }
}
