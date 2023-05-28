use crate::models::XMLEvent;
use crate::utils;
use actix_web::{get, web, HttpResponse};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use log::error;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Deserialize, Debug)]
pub struct CalendarQueryArgs {
    start: NaiveDateTime, // eg. 2022-01-01T00:00:00
    end: NaiveDateTime,   // eg. 2022-01-07T00:00:00
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(calendar_handler);
}

fn get_calendar_url(requested_key: &str, conn: &mut PgConnection) -> QueryResult<String> {
    use crate::schema::rooms::dsl::*;
    let room = rooms
        .filter(key.eq(requested_key))
        .first::<crate::models::Room>(conn)?;
    Ok(format!(
        "https://campus.tum.de/tumonline/wbKalender.wbRessource?pResNr={}",
        room.tumonline_calendar_id
    ))
}

fn get_entries(
    requested_key: &str,
    args: CalendarQueryArgs,
    conn: &mut PgConnection,
) -> QueryResult<Vec<XMLEvent>> {
    use crate::schema::calendar::dsl::*;
    calendar
        .filter(key.eq(&requested_key))
        .filter(dtstart.ge(&args.start))
        .filter(dtend.le(&args.end))
        .load::<XMLEvent>(conn)
}

#[get("/{id}")]
pub async fn calendar_handler(
    params: web::Path<String>,
    web::Query(args): web::Query<CalendarQueryArgs>,
) -> HttpResponse {
    let id = params.into_inner();
    let conn = &mut utils::establish_connection();
    let results = get_entries(&id, args, conn);
    let calendar_url = get_calendar_url(&id, conn);
    match (results, calendar_url) {
        (Ok(results), Ok(calendar_url)) => {
            let last_sync = results.iter().map(|e| e.last_scrape).min().unwrap();
            let events = results.into_iter().map(Event::from).collect();
            HttpResponse::Ok().json(Events {
                events,
                last_sync,
                calendar_url,
            })
        }
        (Err(e), _) => {
            error!("Error loading calendar entries: {e:?}");
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Error loading calendar")
        }
        (_, Err(e)) => {
            error!("Error loading calendar_url: {e:?}");
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Error loading calendar")
        }
    }
}

#[derive(Serialize, Debug)]
struct Events {
    events: Vec<Event>,
    last_sync: NaiveDateTime,
    calendar_url: String,
}

#[derive(Serialize, Debug)]
struct Event {
    id: i32,
    title: String,
    start: NaiveDateTime,
    end: NaiveDateTime,
    entry_type: EventType,
    detailed_entry_type: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "lowercase")]
enum EventType {
    Lecture,
    Exercise,
    Exam,
    Barred,
    Other,
}
impl EventType {
    fn from(xml_event: &XMLEvent) -> (Self, String) {
        // only used for the lecture type
        let course_type_name = xml_event
            .course_type_name
            .clone()
            .unwrap_or_else(|| "Course type is unknown".to_string());
        match xml_event.single_event_type_id.as_str() {
            "SPERRE" => return (EventType::Barred, "".to_string()),
            "PT" => return (EventType::Exam, "".to_string()),
            "P" => return (EventType::Lecture, course_type_name), // Prüfung (geplant) is sometimes used for lectures
            _ => {}
        }
        match xml_event.event_type_id.as_str() {
            "LV" => (EventType::Lecture, course_type_name),
            "PT" => (EventType::Exam, "".to_string()),
            "EX" => (EventType::Exercise, "".to_string()),
            _ => match &xml_event.event_type_name {
                Some(event_type_name) => (
                    EventType::Other,
                    format!("{}: {}", xml_event.single_event_type_name, event_type_name),
                ),
                None => (EventType::Other, xml_event.single_event_type_name.clone()),
            },
        }
    }
}

impl From<XMLEvent> for Event {
    fn from(xml_event: XMLEvent) -> Self {
        let (entry_type, detailed_entry_type) = EventType::from(&xml_event);
        let title = xml_event.event_title;
        Self {
            id: xml_event.single_event_id,
            title,
            start: xml_event.dtstart,
            end: xml_event.dtend,
            entry_type,
            detailed_entry_type,
        }
    }
}
