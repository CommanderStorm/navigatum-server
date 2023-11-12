
#[derive(Debug, Clone)]
pub struct DBRoomEntry {
    pub key: String,
    pub name: String,
    pub tumonline_room_nr: Option<i32>,
    pub type_: String,
    pub type_common_name: String,
    pub lat: f32,
    pub lon: f32,
    pub data: String,
}

#[derive(Debug, Clone)]
pub struct DBRoomKeyAlias {
    pub key: String,
    pub visible_id: String,
    pub type_: String,
}
