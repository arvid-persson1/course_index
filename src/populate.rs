use std::{collections::HashMap, time::Duration};
use askama::filters::capitalize;

use futures::future::join_all;
use itertools::Itertools;
use kuchikiki::iter::NodeIterator;
use kuchikiki::parse_html;
use kuchikiki::traits::TendrilSink;
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::{Client, IntoUrl};
use sqlx::{Error as SqlxError, PgPool, query};
use tokio::time::{sleep, timeout};

use courselib::{Course, Difficulty, Language, Pace, Site};

const FETCH_TRIES: u8 = 5;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_RETRY_DELAY: Duration = Duration::from_secs(5);

lazy_static! {
    static ref CLIENT: Client = Client::new();
    static ref COUNT_PAT: Regex = Regex::new(r"(\d+) träffar").expect("failed to parse regex");
    static ref POINTS_DIFF_CODE_PAT: Regex = Regex::new(r"(\d+(?:,\d)?) (?:(?:högskole)|(?:förutbildnings))poäng, ([^,]+), ([A-Z][A-Z\d]\d{3}[A-Z])").expect("failed to parse regex");
    static ref PERIOD_MODULES_PAT: Regex = Regex::new(r"Period ([1-4]) - ([1-4]), v. \d+ \d+ - v. \d+ \d+, (.+)").expect("failed to parse regex");
}

// This code is allowed to panic since it will only run internally, and we don't want to put incorrect or incomplete data into the database.
// It's also allowed to be difficult to maintain since it will only run once every so often to manually update the database.

#[tokio::main]
async fn main() {
    eprintln!("categories and insert conflicts currently unhandled");

    println!("starting");

    let courses = join_all(fetch_course_pages()
        .await
        .map(parse_page))
        .await;

    println!("all courses processed");

    match insert(courses).await {
        Ok(()) => println!("database population successful"),
        Err(e) => {
            eprintln!("{:#?}", e);
            panic!();
        }
    }
}

type Html = String;
type Url = String;

async fn fetch_html<U: IntoUrl + Clone>(url: U) -> Html {
    let mut tries = FETCH_TRIES;

    while tries > 0 {
        if let Ok(Ok(response)) = timeout(FETCH_TIMEOUT, CLIENT.get(url.clone()).send()).await {
            if let Ok(html) = response.text().await {
                return html;
            }
        }

        tries -= 1;
        sleep(FETCH_RETRY_DELAY).await;
    }

    panic!("failed to get html");
}

async fn fetch_course_pages() -> impl Iterator<Item = Url> {
    // Currently, the first page is fetched twice.

    let count = {
        let first_page = fetch_html(r#"https://www.ltu.se/utbildning/sok-bland-vara-program-och-kurser?educationType=%5B"Kurs"%5D"#).await;
        let node = parse_html().one(first_page)
            .select_first(".TZWHL6tsKFG7e3QLS0Ve")
            .expect("failed to find count wrapper");
        let value = COUNT_PAT.captures(&node.text_contents())
            .expect("failed regex match for count")
            .get(1)
            .expect("failed regex capture for count")
            .as_str()
            .parse::<u16>()
            .expect("failed int parse");
        (value - 1) / 20
    };

    let pages = join_all((0..=count)
        .map(|index| format!(r#"https://www.ltu.se/utbildning/sok-bland-vara-program-och-kurser?educationType=%5B"Kurs"%5D&p={}"#, index))
        .map(fetch_html))
        .await;

    parse_html().from_iter(pages)
        .select(".ZjffZkYcXrC8qp0Drppg")
        .expect("failed to find course pages")
        .map(|item| item.attributes
            .borrow()
            .get("href")
            .expect("failed to find course page links")
            .to_owned())
        .unique()
        .map(|url| format!("https://www.ltu.se{}", url))
}

async fn parse_page(url: Url) -> Course {
    let node = parse_html().one(fetch_html(url.clone()).await);

    let (points, difficulty, code) = {
        let raw = node.select_first(".PT2hF8CC4ZkIu8gQjcXZ")
            .expect("failed to find points, difficulty, code")
            .text_contents();

        let (points, difficulty, code) = POINTS_DIFF_CODE_PAT.captures(&raw)
            .expect("failed regex match for points, difficulty, code")
            .iter()
            .skip(1)
            .map(|m| m.expect("failed regex capture for points, difficulty, code").as_str())
            .collect_tuple()
            .expect("wrong number of items found for points, difficulty, code");

        (
            points
                .replace(',', ".")
                .parse()
                .expect("failed points parse"),
            capitalize(difficulty)
                .unwrap()
                .parse()
                .expect("failed difficulty parse"),
            code
                .to_owned()
        )
    };

    let name_se = node.select_first(".heading")
        .expect("failed to find swedish header")
        .text_contents();

    let name_en = {
        let url_en = node.select_first("#svid12_54e1ff71188bd846477119d>p>a")
            .ok()
            .map(|button_en| button_en
                .attributes.borrow()
                .get("href")
                .expect("failed to find english page link")
                .to_owned()
            );

        // Workaround since async closures are unstable.
        if let Some(url_en) = url_en {
            let node_en = parse_html().one(fetch_html(format!("https://www.ltu.se{}", url_en)).await);
            Some(node_en.select_first(".heading")
                .expect("failed to find english header")
                .text_contents())
        } else {
            None
        }
    };

    let mut fields = node.select(".lhgan24SXOCFJ2gbcDaQ")
        .expect("failed to find fields")
        .map(|field| {
            let mut items = field
                .as_node()
                .descendants()
                .text_nodes()
                //.filter(|n| n.first_child().is_none())
                .map(|n| n.as_node().text_contents());
            let key = items
                .next()
                .expect("no first field");
            let value = items
                .skip(1)
                .join("\n");
            (key, value)
        })
        .collect::<HashMap<_, _>>();

    let (period_start, period_end, modules) = {
        if let Some(field) = fields.remove("Period") {
            let (period_start, period_end, modules) = PERIOD_MODULES_PAT.captures(&field)
                .expect("failed regex match for period, modules")
                .iter()
                .skip(1)
                .map(|m| m.expect("failed regex capture for period, modules").as_str())
                .collect_tuple()
                .expect("wrong number of items found for period, modules");

            (
                Some(period_start
                    .parse()
                    .expect("failed int parse")),
                Some(period_end
                    .parse()
                    .expect("failed int parse")),
                Some(modules
                    .to_owned())
            )
        } else {
            (None, None, None)
        }
    };

    let site = fields.remove("Studieort").map(|s| s.parse().expect("failed site parse"));
    let pace = fields.remove("Studieform").map(|s| s.parse().expect("failed pace parse"));
    let language = fields.remove("Språk").map(|s| s.parse().expect("failed language parse"));
    let prerequisites = fields.remove("Förkunskapskrav");
    let register_info = fields.remove("Sökinformation");
    let conduct = fields.remove("Genomförande");

    println!(r"processed {}", code);

    Course {
        code,
        name_se,
        name_en,
        url,
        points,
        pace,
        prerequisites,
        register_info,
        modules,
        period_start,
        period_end,
        site,
        language,
        difficulty,
        categories: Default::default(),
        conduct
    }
}

macro_rules! destruct_vec {
    ($it:expr, $($field:ident),*) => {{
        $(
            let mut $field = Vec::with_capacity($it.len());
        )*

        for course in $it.into_iter() {
            $(
                $field.push(course.$field);
            )*
        }

        ($($field),*)
    }};
}

async fn insert(courses: Vec<Course>) -> Result<(), SqlxError> {
    let connection = PgPool::connect(include_str!("../connection_string")).await?;

    let (codes, names_se, names_en, urls, points, paces, prerequisites, register_info, modules, period_starts, period_ends, sites, languages, difficulties, conducts) =
        destruct_vec!(courses, code, name_se, name_en, url, points, pace, prerequisites, register_info, modules, period_start, period_end, site, language, difficulty, conduct);

    // TODO: handle insert conflict
    query!(
        "INSERT INTO courses (code, name_se, name_en, url, points, pace, prerequisites, register_info, modules, period_start, period_end, site, language, difficulty, conduct)
        SELECT * FROM UNNEST($1::CHARACTER(6)[], $2::TEXT[], $3::TEXT[], $4::TEXT[], $5::REAL[], $6::pace_enum[], $7::TEXT[], $8::TEXT[], $9::TEXT[], $10::SMALLINT[], $11::SMALLINT[], $12::site_enum[], $13::language_enum[], $14::difficulty_enum[], $15::TEXT[])",
        &codes,
        &names_se,
        names_en as Vec<Option<String>>,
        &urls,
        &points,
        paces as Vec<Option<Pace>>,
        prerequisites as Vec<Option<String>>,
        register_info as Vec<Option<String>>,
        modules as Vec<Option<String>>,
        period_starts as Vec<Option<i16>>,
        period_ends as Vec<Option<i16>>,
        sites as Vec<Option<Site>>,
        languages as Vec<Option<Language>>,
        difficulties as Vec<Difficulty>,
        conducts as Vec<Option<String>>,
    )
        .execute(&connection)
        .await?;

    Ok(())
}
