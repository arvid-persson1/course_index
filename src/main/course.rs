use actix_web::{get, HttpResponse, Responder, web};
use askama::filters::capitalize;
use askama::Template;
use serde::Deserialize;
use sqlx::query_as;

use courselib::{Course, Pace};

use super::CONNECTION;

#[derive(Deserialize, Debug, Clone)]
struct CourseQuery {
    id: i32,
}

#[derive(Template)]
#[template(path = "../html/course.html")]
struct CourseTemplate {
    title: String,
    code: String,
    name_se: String,
    name_en: Option<String>,
    url: String,
    points: String,
    pace: Option<Pace>,
    prerequisites: Option<String>,
    register_info: Option<String>,
    modules: Option<String>,
    periods: Option<String>,
    site: Option<String>,
    language: Option<String>,
    difficulty: String,
    categories: Vec<String>,
    conduct: Option<String>,
}

impl From<Course> for CourseTemplate {
    fn from(value: Course) -> Self {
        Self {
            title: format!("{} {}", value.code, value.name_se),
            code: value.code,
            name_en: value.name_en.filter(|n| n != &value.name_se),
            name_se: value.name_se,
            url: value.url,
            points: value.points.to_string().replace('.', ","),
            pace: value.pace,
            prerequisites: value.prerequisites,
            register_info: value.register_info,
            modules: value.modules.map(|m| capitalize(m).unwrap()),
            periods: match (value.period_start, value.period_end) {
                (Some(start), Some(end)) if start != end =>
                    Some(format!("{} till {}", start, end)),
                (Some(start), _) | (None, Some(start)) =>
                    Some(start.to_string()),
                (None, None) =>
                    None
            },
            site: value.site.map(|s| s.to_string()),
            language: value.language.map(|l| l.to_string()),
            difficulty: value.difficulty.to_string(),
            categories: value.categories.iter().map(|c| c.to_string()).collect(),
            conduct: value.conduct,
        }
    }
}

#[get("/course")]
async fn page(query: web::Query<CourseQuery>) -> impl Responder {
    let res = query_as!(
        Course,
        r#"SELECT code, name_se, name_en, url, points, pace as "pace: _", prerequisites, register_info, modules, period_start, period_end, site as "site: _", language as "language: _", difficulty as "difficulty: _", categories as "categories: _", conduct
        FROM courses
        WHERE id = $1"#,
        query.id
    )
        .fetch_optional(CONNECTION.get().unwrap())
        .await;

    match res {
        Ok(Some(course)) => {
            HttpResponse::Ok()
                .body(Into::<CourseTemplate>::into(course).render().unwrap())
        }
        Ok(None) => {
            // that id doesn't exist
            HttpResponse::NotFound().finish() // FIXME
        }
        Err(e) => {
            eprintln!("{}", e);
            // sql error
            HttpResponse::InternalServerError().finish() // FIXME
        }
    }
}
