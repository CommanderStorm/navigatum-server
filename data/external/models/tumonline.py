import json

from external.models.common import PydanticConfiguration, RESULTS
from pydantic.dataclasses import dataclass


@dataclass(config=PydanticConfiguration)
# pylint: disable-next=too-many-instance-attributes
class ExtendedRoomData:
    address: str
    building: str
    zip_code_location: str
    room_number: str
    floor_number: str
    floor_type: str
    area_m2: float
    architect_room_nr: str
    additional_description: str
    purpose: str
    wheelchair_spaces: int
    standing_places: int
    seats: int


@dataclass(config=PydanticConfiguration)
# pylint: disable-next=too-many-instance-attributes
class Room:
    address: str
    address_link: str
    alt_name: str
    arch_name: str
    b_area_id: int
    b_filter_id: int
    calendar: str | None
    list_index: str
    op_link: str
    operator: str
    plz_place: str
    room_link: str
    roomcode: str
    usage: int
    extended: ExtendedRoomData | None = None

    @classmethod
    def load_all(cls) -> list["Room"]:
        """Load all tumonline.Room's"""
        with open(RESULTS / "rooms_tumonline.json", encoding="utf-8") as file:
            return [cls(**item) for item in json.load(file)]


@dataclass(config=PydanticConfiguration)
class Building:
    area_id: int
    filter_id: int
    name: str

    @classmethod
    def load_all(cls) -> list["Building"]:
        """Load all tumonline.Building's"""
        with open(RESULTS / "buildings_tumonline.json", encoding="utf-8") as file:
            return [cls(**item) for item in json.load(file)]


@dataclass(config=PydanticConfiguration)
class Organisation:
    # pylint: disable-next=invalid-name
    id: int
    code: str
    name: str
    path: str

    @classmethod
    def load_all_for(cls, lang: str) -> dict[str, "Organisation"]:
        """Load all tumonline.Organisation's for a specific language"""
        with open(RESULTS / f"orgs-{lang}_tumonline.json", encoding="utf-8") as file:
            return {key: cls(**item) for key, item in json.load(file).items()}


@dataclass(config=PydanticConfiguration)
class Usage:
    # pylint: disable-next=invalid-name
    id: int
    din_277: str
    name: str

    @classmethod
    def load_all(cls) -> dict[int, "Usage"]:
        """Load all tumonline.Usage's"""
        with open(RESULTS / "usages_tumonline.json", encoding="utf-8") as file:
            return {item["id"]: cls(**item) for item in json.load(file)}
