use crate::localisation;
use actix_web::{get, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, error};
use valhalla_client::costing::auto::AutoCostingOptions;
use valhalla_client::costing::bicycle::BicycleCostingOptions;
use valhalla_client::costing::motorcycle::MotorcycleCostingOptions;
use valhalla_client::costing::multimodal::MultimodalCostingOptions;
use valhalla_client::costing::pedestrian::PedestrianCostingOptions;
use valhalla_client::route::{
    Leg, Maneuver, ManeuverType, Summary, TransitInfo, TransitStop, TransitStopType, TravelMode,
    Trip,
};
use valhalla_client::{costing::Costing, route, route::Location, Valhalla};

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, utoipa::ToSchema)]
struct Coordinate {
    /// Latitude
    #[schema(example = 48.26244490906312)]
    lat: f64,
    /// Longitude
    #[schema(example = 48.26244490906312)]
    lon: f64,
}
// todo
//impl From<ShapePoint> for Coordinate{
//    fn from(value: ShapePoint) -> Self {
//        Coordinate{lon:value.lon ,lat:value.lat }
//    }
//}

#[derive(Deserialize, Clone, Debug, PartialEq, utoipa::ToSchema)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum RequestedLocation {
    /// Either an
    /// - external address which was looked up or
    /// - the users current location  
    Coordinate(Coordinate),
    /// Our (uni internal) key for location identification
    Location(String),
}
impl RequestedLocation {
    async fn try_resolve_coordinates(&self, pool: &PgPool) -> anyhow::Result<Option<Coordinate>> {
        match self {
            RequestedLocation::Coordinate(coords) => Ok(Some(*coords)),
            RequestedLocation::Location(key) => {
                let coords = sqlx::query_as!(
                    Coordinate,
                    r#"SELECT lat,lon
                    FROM de
                    WHERE key = $1 and
                          lat IS NOT NULL and
                          lon IS NOT NULL"#,
                    key
                )
                .fetch_optional(pool)
                .await?;
                Ok(coords)
            }
        }
    }
}

/// Transport mode the user wants to use
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum CostingRequest {
    Pedestrian,
    Bicycle,
    Motorcycle,
    Car,
    PublicTransit,
}
impl From<CostingRequest> for Costing {
    fn from(value: CostingRequest) -> Self {
        match value {
            CostingRequest::Pedestrian => Costing::Pedestrian(PedestrianCostingOptions::builder()),
            CostingRequest::Bicycle => Costing::Bicycle(BicycleCostingOptions::builder()),
            CostingRequest::Motorcycle => Costing::Motorcycle(MotorcycleCostingOptions::builder()),
            CostingRequest::Car => Costing::Auto(AutoCostingOptions::builder()),
            CostingRequest::PublicTransit => {
                Costing::Multimodal(MultimodalCostingOptions::builder())
            }
        }
    }
}

#[derive(Deserialize, Debug, utoipa::ToSchema, utoipa::IntoParams)]
struct RoutingRequest {
    #[serde(flatten)]
    lang: localisation::LangQueryArgs,
    /// Start of the route
    from: RequestedLocation,
    /// Destination of the route
    to: RequestedLocation,
    /// Transport mode the user wants to use
    route_costing: CostingRequest,
}

/// Routing requests
///
/// The user specifies using provided origin (`from`) and destination (`to`) locations and a transport mode (`route_costing`) to tune their routing between the two locations.
/// The costing is fine-tuned by the server side accordingly.
///
/// Internally, this endpoint relies on
/// - [Valhalla](https://github.com/valhalla/valhalla) for routing for route calculation
/// - our database to resolve ids.
///   
///   You will need to look the ids up via [`/api/search`](#tag/locations/operation/search_handler) beforehand.
///   **Note:** [`/api/search`](#tag/locations/operation/search_handler) does support both university internal routing and external addressing.
///
/// **In the future (i.e. public transit routing currently is not implemented)**, it will als rely on either
/// - [OpenTripPlanner2](https://www.opentripplanner.org/) or
/// - [Motis](https://github.com/motis-project/motis)
#[utoipa::path(
    tags=["maps"],
    params(RoutingRequest),
    responses(
        (status = 200, description = "**Routing solution**", body=RoutingResponse, content_type = "application/json"),
        (status = 404, description = "**Not found.** The requested location does not exist", body = String, content_type = "text/plain", example = "Not found"),
    )
)]
#[get("/api/maps/route")]
pub async fn route_handler(
    args: web::Query<RoutingRequest>,
    data: web::Data<crate::AppData>,
) -> HttpResponse {
    let from = args.from.try_resolve_coordinates(&data.pool).await;
    let to = args.from.try_resolve_coordinates(&data.pool).await;
    let (from, to) = match (from, to) {
        (Ok(Some(from)), Ok(Some(to))) => (from, to),
        (Ok(None), _) | (_, Ok(None)) => {
            return HttpResponse::NotFound()
                .content_type("text/plain")
                .body("Not found");
        }
        (Err(e), _) | (_, Err(e)) => {
            error!(from=?args.from,to=?args.to,error = ?e,"could not resolve into coordinates");
            return HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Failed to resolve key");
        }
    };
    debug!(?from, ?to, "routing request");
    let base_url = "https://nav.tum.de/valhalla".parse().unwrap();
    let valhalla = Valhalla::new(base_url);
    let request = route::Manifest::builder()
        .locations([
            Location::new(from.lat as f32, from.lon as f32),
            Location::new(to.lat as f32, to.lon as f32),
        ])
        .costing(Costing::from(args.route_costing))
        .language(if args.lang.should_use_english() {
            "en-US"
        } else {
            "de-DE"
        });

    let Ok(response) = valhalla.route(request) else {
        return HttpResponse::InternalServerError()
            .content_type("text/plain")
            .body("Could not generate a route, please try again later");
    };
    debug!(routing_solution=?response,"got routing solution");

    HttpResponse::Ok().json(RoutingResponse::from(response))
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
struct RoutingResponse {
    legs: Vec<LegResponse>,
    summary: SummaryResponse,
}
impl From<Trip> for RoutingResponse {
    fn from(value: Trip) -> Self {
        RoutingResponse {
            legs: value.legs.into_iter().map(LegResponse::from).collect(),
            summary: SummaryResponse::from(value.summary),
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
struct SummaryResponse {
    time: f64,
    length: f64,
    has_toll: bool,
    has_highway: bool,
    has_ferry: bool,
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
}
impl From<Summary> for SummaryResponse {
    fn from(value: Summary) -> Self {
        SummaryResponse {
            time: value.time,
            length: value.length,
            has_toll: value.has_toll,
            has_highway: value.has_highway,
            has_ferry: value.has_ferry,

            min_lat: value.min_lat,
            min_lon: value.min_lon,
            max_lat: value.max_lat,
            max_lon: value.max_lon,
        }
    }
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
struct LegResponse {
    summary: SummaryResponse,

    maneuvers: Vec<ManeuverResponse>,
    //todo
    //shape: Vec<Coordinate>,
}
impl From<Leg> for LegResponse {
    fn from(value: Leg) -> Self {
        LegResponse {
            summary: SummaryResponse::from(value.summary),
            maneuvers: value
                .maneuvers
                .into_iter()
                .map(ManeuverResponse::from)
                .collect(),
            // todo
            //            shape: value.shape.into_iter().map(Coordinate::from).collect(),
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
struct ManeuverResponse {
    r#type: ManeuverTypeResponse,

    instruction: String,

    /// Text suitable for use as a verbal alert in a navigation application.
    ///
    /// The transition alert instruction will prepare the user for the forthcoming transition.
    ///
    /// Example: "Turn right onto North Prince Street"
    verbal_transition_alert_instruction: Option<String>,

    /// Text suitable for use as a verbal message immediately prior to the maneuver transition.
    ///
    /// Example: "Turn right onto North Prince Street, U.S. 2 22"
    verbal_pre_transition_instruction: Option<String>,
    /// Text suitable for use as a verbal message immediately after the maneuver transition.
    ///
    /// Example: "Continue on U.S. 2 22 for 3.9 miles"
    verbal_post_transition_instruction: Option<String>,

    /// List of street names that are consistent along the entire nonobvious maneuver
    street_names: Option<Vec<String>>,

    /// When present, these are the street names at the beginning (transition point) of the
    /// nonobvious maneuver (if they are different than the names that are consistent along the
    /// entire nonobvious maneuver).
    begin_street_names: Option<Vec<String>>,
    /// Estimated time along the maneuver in seconds.
    time: f64,
    /// Maneuver length in the [`super::Units`] specified via [`Manifest::units`]
    length: f64,
    /// Index into the list of shape points for the start of the maneuver.
    begin_shape_index: usize,
    /// Index into the list of shape points for the end of the maneuver.
    end_shape_index: usize,
    /// `true` if a toll booth is encountered on this maneuver.
    toll: Option<bool>,
    /// `true` if a highway is encountered on this maneuver.
    highway: Option<bool>,
    /// `true` if the maneuver is unpaved or rough pavement, or has any portions that have rough
    /// pavement.
    rough: Option<bool>,
    /// `true` if a gate is encountered on this maneuver.
    gate: Option<bool>,
    /// `true` if a ferry is encountered on this maneuver.
    ferry: Option<bool>,
    /// The spoke to exit roundabout after entering.
    roundabout_exit_count: Option<i64>,
    /// Written depart time instruction.
    ///
    /// Typically used with a transit maneuver, such as "Depart: 8:04 AM from 8 St - NYU".
    depart_instruction: Option<String>,
    /// Text suitable for use as a verbal depart time instruction.
    ///
    /// Typically used with a transit maneuver, such as "Depart at 8:04 AM from 8 St - NYU".
    verbal_depart_instruction: Option<String>,
    /// Written arrive time instruction.
    ///
    /// Typically used with a transit maneuver, such as "Arrive: 8:10 AM at 34 St - Herald Sq".
    arrive_instruction: Option<String>,
    /// Text suitable for use as a verbal arrive time instruction.
    ///
    /// Typically used with a transit maneuver, such as "Arrive at 8:10 AM at 34 St - Herald Sq".
    verbal_arrive_instruction: Option<String>,
    /// Contains the attributes that describe a specific transit route.
    ///
    /// See [`TransitInfo`] for details.
    transit_info: Option<TransitInfoResponse>,
    /// Contains the attributes that describe a specific transit stop.
    ///
    /// `true` if [`Self::verbal_pre_transition_instruction`] has been appended with
    /// the verbal instruction of the next maneuver.
    verbal_multi_cue: Option<bool>,

    /// Travel mode
    travel_mode: TravelModeResponse,
}
impl From<Maneuver> for ManeuverResponse {
    fn from(value: Maneuver) -> Self {
        ManeuverResponse {
            r#type: ManeuverTypeResponse::from(value.type_),
            instruction: value.instruction,
            verbal_transition_alert_instruction: value.verbal_transition_alert_instruction,
            verbal_pre_transition_instruction: value.verbal_pre_transition_instruction,
            verbal_post_transition_instruction: value.verbal_post_transition_instruction,
            street_names: value.street_names,
            begin_street_names: value.begin_street_names,
            time: value.time,
            length: value.length,
            begin_shape_index: value.begin_shape_index,
            end_shape_index: value.end_shape_index,
            toll: value.toll,
            highway: value.highway,
            rough: value.rough,
            gate: value.gate,
            ferry: value.ferry,
            roundabout_exit_count: value.roundabout_exit_count,
            depart_instruction: value.depart_instruction,
            verbal_depart_instruction: value.verbal_depart_instruction,
            arrive_instruction: value.arrive_instruction,
            verbal_arrive_instruction: value.verbal_arrive_instruction,
            transit_info: if let Some(info) = value.transit_info {
                Some(TransitInfoResponse::from(info))
            } else {
                None
            },
            verbal_multi_cue: value.verbal_multi_cue,
            travel_mode: TravelModeResponse::from(value.travel_mode),
        }
    }
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum ManeuverTypeResponse {
    None,
    Start,
    StartRight,
    StartLeft,
    Destination,
    DestinationRight,
    DestinationLeft,
    Becomes,
    Continue,
    SlightRight,
    Right,
    SharpRight,
    UturnRight,
    UturnLeft,
    SharpLeft,
    Left,
    SlightLeft,
    RampStraight,
    RampRight,
    RampLeft,
    ExitRight,
    ExitLeft,
    StayStraight,
    StayRight,
    StayLeft,
    Merge,
    RoundaboutEnter,
    RoundaboutExit,
    FerryEnter,
    FerryExit,
    Transit,
    TransitTransfer,
    TransitRemainOn,
    TransitConnectionStart,
    TransitConnectionTransfer,
    TransitConnectionDestination,
    PostTransitConnectionDestination,
    MergeRight,
    MergeLeft,
    ElevatorEnter,
    StepsEnter,
    EscalatorEnter,
    BuildingEnter,
    BuildingExit,
}
impl From<ManeuverType> for ManeuverTypeResponse {
    fn from(value: ManeuverType) -> Self {
        match value {
            ManeuverType::None => Self::None,
            ManeuverType::Start => Self::Start,
            ManeuverType::StartRight => Self::StartRight,
            ManeuverType::StartLeft => Self::StartLeft,
            ManeuverType::Destination => Self::Destination,
            ManeuverType::DestinationRight => Self::DestinationRight,
            ManeuverType::DestinationLeft => Self::DestinationLeft,
            ManeuverType::Becomes => Self::Becomes,
            ManeuverType::Continue => Self::Continue,
            ManeuverType::SlightRight => Self::SlightRight,
            ManeuverType::Right => Self::Right,
            ManeuverType::SharpRight => Self::SharpRight,
            ManeuverType::UturnRight => Self::UturnRight,
            ManeuverType::UturnLeft => Self::UturnLeft,
            ManeuverType::SharpLeft => Self::SharpLeft,
            ManeuverType::Left => Self::Left,
            ManeuverType::SlightLeft => Self::SlightLeft,
            ManeuverType::RampStraight => Self::RampStraight,
            ManeuverType::RampRight => Self::RampRight,
            ManeuverType::RampLeft => Self::RampLeft,
            ManeuverType::ExitRight => Self::ExitRight,
            ManeuverType::ExitLeft => Self::ExitLeft,
            ManeuverType::StayStraight => Self::StayStraight,
            ManeuverType::StayRight => Self::StayRight,
            ManeuverType::StayLeft => Self::StayLeft,
            ManeuverType::Merge => Self::Merge,
            ManeuverType::RoundaboutEnter => Self::RoundaboutEnter,
            ManeuverType::RoundaboutExit => Self::RoundaboutExit,
            ManeuverType::FerryEnter => Self::FerryEnter,
            ManeuverType::FerryExit => Self::FerryExit,
            ManeuverType::Transit => Self::Transit,
            ManeuverType::TransitTransfer => Self::TransitTransfer,
            ManeuverType::TransitRemainOn => Self::TransitRemainOn,
            ManeuverType::TransitConnectionStart => Self::TransitConnectionStart,
            ManeuverType::TransitConnectionTransfer => Self::TransitConnectionTransfer,
            ManeuverType::TransitConnectionDestination => Self::TransitConnectionDestination,
            ManeuverType::PostTransitConnectionDestination => {
                Self::PostTransitConnectionDestination
            }
            ManeuverType::MergeRight => Self::MergeRight,
            ManeuverType::MergeLeft => Self::MergeLeft,
            ManeuverType::ElevatorEnter => Self::ElevatorEnter,
            ManeuverType::StepsEnter => Self::StepsEnter,
            ManeuverType::EscalatorEnter => Self::EscalatorEnter,
            ManeuverType::BuildingEnter => Self::BuildingEnter,
            ManeuverType::BuildingExit => Self::BuildingExit,
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]

struct TransitInfoResponse {
    /// Global transit route identifier.
    onestop_id: String,
    /// Short name describing the transit route
    ///
    /// Example: "N"
    short_name: String,
    /// Long name describing the transit route
    ///
    /// Example: "Broadway Express"
    long_name: String,
    /// The sign on a public transport vehicle that identifies the route destination to passengers.
    ///
    /// Example: "ASTORIA - DITMARS BLVD"
    headsign: String,
    /// The numeric color value associated with a transit route.
    ///
    /// The value for yellow would be "16567306".
    color: i32,
    /// The numeric text color value associated with a transit route.
    ///
    /// The value for black would be "0".
    text_color: String,
    /// The description of the transit route
    ///
    /// Example: "Trains operate from Ditmars Boulevard, Queens, to Stillwell Avenue, Brooklyn, at all times
    /// N trains in Manhattan operate along Broadway and across the Manhattan Bridge to and from Brooklyn.
    /// Trains in Brooklyn operate along 4th Avenue, then through Borough Park to Gravesend.
    /// Trains typically operate local in Queens, and either express or local in Manhattan and Brooklyn,
    /// depending on the time. Late night trains operate via Whitehall Street, Manhattan.
    /// Late night service is local"
    description: String,
    /// Global operator/agency identifier.
    operator_onestop_id: String,
    /// Operator/agency name
    ///
    /// Short name is used over long name.
    ///
    /// Example: "BART", "King County Marine Division", and so on.
    operator_name: String,
    /// Operator/agency URL
    ///
    /// Example: `http://web.mta.info/`.
    operator_url: String,
    /// A list of the stops/stations associated with a specific transit route.
    ///
    /// See [`TransitStop`] for details.
    transit_stops: Vec<TransitStopResponse>,
}
impl From<TransitInfo> for TransitInfoResponse {
    fn from(value: TransitInfo) -> Self {
        TransitInfoResponse {
            onestop_id: value.onestop_id,
            short_name: value.short_name,
            long_name: value.long_name,
            headsign: value.headsign,
            color: value.color,
            text_color: value.text_color,
            description: value.description,
            operator_onestop_id: value.operator_onestop_id,
            operator_name: value.operator_name,
            operator_url: value.operator_url,
            transit_stops: value
                .transit_stops
                .into_iter()
                .map(TransitStopResponse::from)
                .collect(),
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum TravelModeResponse {
    Drive,
    Pedestrian,
    Bicycle,
    PublicTransit,
}
impl From<TravelMode> for TravelModeResponse {
    fn from(value: TravelMode) -> Self {
        match value {
            TravelMode::Drive => Self::Drive,
            TravelMode::Pedestrian => Self::Pedestrian,
            TravelMode::Bicycle => Self::Bicycle,
            TravelMode::Transit => Self::PublicTransit,
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
struct TransitStopResponse {
    r#type: TransitStopTypeResponse,
    /// Name of the stop or station
    ///
    /// Example: "14 St - Union Sq"
    name: String,
    /// Arrival date and time
    arrival_date_time: chrono::NaiveDateTime,
    /// Departure date and time
    departure_date_time: chrono::NaiveDateTime,
    /// `true` if this stop is a marked as a parent stop.
    is_parent_stop: bool,
    /// `true` if the times are based on an assumed schedule because the actual schedule is not
    /// known.
    assumed_schedule: bool,
    /// Latitude of the transit stop in degrees.
    lat: f64,
    /// Longitude of the transit stop in degrees.
    lon: f64,
}
impl From<TransitStop> for TransitStopResponse {
    fn from(value: TransitStop) -> Self {
        TransitStopResponse {
            r#type: TransitStopTypeResponse::from(value.type_),
            name: value.name,
            arrival_date_time: value.arrival_date_time,
            departure_date_time: value.departure_date_time,
            is_parent_stop: value.is_parent_stop,
            assumed_schedule: value.assumed_schedule,
            lat: value.lat,
            lon: value.lon,
        }
    }
}
#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum TransitStopTypeResponse {
    /// Simple stop
    Stop,
    /// Station
    Station,
}
impl From<TransitStopType> for TransitStopTypeResponse {
    fn from(value: TransitStopType) -> Self {
        match value {
            TransitStopType::Stop => Self::Stop,
            TransitStopType::Station => Self::Station,
        }
    }
}
