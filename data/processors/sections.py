from utils import TranslatableStr as _


def extract_calendar_urls(data):
    """Extracts the calendar from the tumonline data sets it to the proper value."""
    for entry in data.values():
        if entry.get("tumonline_data", {}).get("calendar", None):
            url = f"https://campus.tum.de/tumonline/{entry['tumonline_data']['calendar']}"
            entry["props"]["calendar_url"] = url
        if entry.get("tumonline_data", {}).get("room_link", None):
            url: str = entry["tumonline_data"]["room_link"]
            entry["props"]["tumonline_room_nr"] = int(url.removeprefix("wbRaum.editRaum?pRaumNr="))
        elif entry.get("tumonline_data", {}).get("address_link", None):
            url: str = entry["tumonline_data"]["address_link"]
            entry["props"]["tumonline_room_nr"] = int(url.removeprefix("ris.einzelraum?raumkey="))


def compute_floor_prop(data):
    """
    Create a human readable floor information prop that takes into account
    special floor numbering systems of buildings.
    """
    for _id, entry in data.items():
        if entry["type"] in {"root", "area", "site", "campus", "virtual_room"}:
            continue

        parent_type = data[entry["parents"][-1]]["type"]
        if parent_type == "joined_building":
            continue

        if "children_flat" not in entry:
            continue

        # Steps in the assignment of floors:
        # - Collect the relevant information from all rooms within the building.
        # - Extract all floors given by TUMonline. TUMonline is used as the source for
        #   the information about the floor, unless there is a patch. From the floors
        #   in TUMonline we build the list of floors in their physical order.
        # - Infer or get details about each floor to generate a name string and get metadata.
        room_data = _collect_floors_room_data(data, entry)
        floors = _build_sorted_floor_list(entry, room_data)
        floor_details = _get_floor_details(entry, room_data, floors)

        entry.setdefault("props", {})["floors"] = floor_details

        # Now add this floor information to all children
        lookup = { floor["tumonline"]: floor for floor in floor_details }
        for room in room_data:
            room_entry = data[room["id"]]
            room_entry.setdefault("props", {})["floor"] = lookup[room["floor"]]


def _collect_floors_room_data(data, entry):
    """ Collect floors of a (joined_)building """
    missing_cnt = 0
    room_data = []
    for child_id in entry["children_flat"]:
        child = data[child_id]
        if child["type"] == "room" and "ids" in child.get("props", {}):
            roomcode = child["props"]["ids"].get("roomcode", None)

            if "floor_patch" in child.get("generators", {}).get("floors", {}):
                floor = child["generators"]["floors"]["floor_patch"]
            else:
                floor = roomcode.split(".")[1]

            room_data.append({
                "id": child_id,
                "floor": floor,
            })

    return room_data


def _build_sorted_floor_list(entry, room_data):
    """ Build a physically sorted list of floors (using TUMonline floor names) """
    floors = set([room["floor"] for room in room_data])

    def floor_quantifier(floor_name):
        """Assign each floor a virtual ID for sorting """
        if floor_name == "EG":
            return 0
        elif floor_name == "DG":
            return 1000
        elif floor_name.startswith("U"):
            return -10 * int(floor_name[1:])
        elif floor_name.isnumeric():
            return 10 * int(floor_name)
        elif floor_name.startswith("Z"):
            # Default placement: Z1 is below 01 etc.
            return 10 * int(floor_name[1:]) - 5
        elif floor_name == "TP":  # Tiefparterre / Semi-Basement
            # Default placement: below EG
            return -5
        else:
            raise RuntimeError(f"Unknown TUMonline floor name {floor_name}")

    return sorted(floors, key=floor_quantifier)


def _get_floor_details(entry, room_data, floors):
    """ Infer for each floor the metadata and name string """
    floors_details = []

    patches = entry.get("generators", {}).get("floors", {}).get("floor_patches", {})

    eg_index = floors.index("EG") if "EG" in floors else 0
    mezzanine_shift = 0
    for i, floor_tumonline in enumerate(floors):
        floor = patches.get(floor_tumonline, {}).get("use_as", floor_tumonline)
        f_id = patches.get(floor_tumonline, {}).get("id", i - eg_index)

        floor_type, floor_abbr, floor_name = \
            _get_floor_name_and_type(f_id, floor, mezzanine_shift)

        floor_name = patches.get(floor_tumonline, {}).get("name", floor_name)

        floors_details.append({
            "id": f_id,
            "floor": floor_abbr,
            "tumonline": floor_tumonline,
            "type": floor_type,
            "name": floor_name,
            "mezzanine_shift": mezzanine_shift,
        })
        if i - eg_index >= 0 and floor.startswith("Z"):
            mezzanine_shift += 1

    return floors_details


def _get_floor_name_and_type(f_id, floor, mezzanine_shift):
    """
    Generate a machine readable floor type and human readable floor name (long & short)
    :param f_id: Floor id (0 for ground floor if there is one, else 0 for the lowest)
    :param floor: Floor name in TUMonline
    :param mezzanine_shift: How many mezzanines are between this floor and floor 0 (only >= 0)
    """
    if floor == "EG":
        floor_type = "ground"
        floor_abbr = "0"
        # TODO: Verify f_id == 0, should be!
        floor_name = _("Erdgeschoss")
    elif floor == "DG":
        floor_type = "roof"
        floor_abbr = str(f_id)
        floor_name = _("Dachgeschoss")
    elif floor == "TP":
        floor_type = "tp"
        floor_abbr = "TP"
        floor_name = _("Tiefparterre")
    elif floor.startswith("U"):
        floor_type = "basement"
        floor_abbr = f"-{floor[1:]}"
        floor_name = _("{n}. Untergeschoss").format(n=floor[1:])
    elif floor.startswith("Z"):
        floor_type = "mezzanine"
        floor_abbr = f"Z{floor[1:]}"
        if f_id == 1:
            floor_name = _("1. Zwischengeschoss, über EG")
        else:
            floor_name = _("{n}. Zwischengeschoss").format(n=floor[1:])
    else:
        floor_type = "upper"
        n = int(floor[1:])
        floor_abbr = str(n)
        if mezzanine_shift == 0:
            floor_name = _("{n}. Obergeschoss").format(n=n)
        elif mezzanine_shift == 1:
            floor_name = _("{n}. OG + 1 Zwischengeschoss").format(n=n)
        else:
            floor_name = _("{n}. OG + {m} Zwischengeschosse").format(n=n, m=mezzanine_shift)

    return floor_type, floor_abbr, floor_name


def compute_props(data):
    """
    Create the "computed" value in "props".
    """
    for _id, entry in data.items():
        if "props" in entry:
            props = entry["props"]
            computed = _gen_computed_props(_id, entry, props)

            # Reformat if required (just to have less verbosity in the code above)
            for i, computed_prop in enumerate(computed):
                if "name" not in computed_prop:
                    computed[i] = {"name": list(computed_prop.keys())[0], "text": list(computed_prop.values())[0]}

            entry["props"]["computed"] = computed


def _append_if_present(props, computed_results, key, human_name):
    if key in props:
        computed_results.append({human_name: str(props[key])})


def _gen_computed_props(_id, entry, props):
    computed = []
    if "ids" in props:
        _append_if_present(props["ids"], computed, "b_id", _("Gebäudekennung"))
        _append_if_present(props["ids"], computed, "roomcode", _("Raumkennung"))
        if "arch_name" in props["ids"]:
            computed.append({_("Architekten-Name"): props["ids"]["arch_name"].split("@")[0]})
    if "floor" in props:
        floor = props["floor"]
        computed.append({_("Stockwerk"): f"{floor['floor']} (" + floor["name"] + ")"})
    if "b_prefix" in entry and entry["b_prefix"] != _id:
        b_prefix = [entry["b_prefix"]] if isinstance(entry["b_prefix"], str) else entry["b_prefix"]
        building_names = ", ".join([p.ljust(4, "x") for p in b_prefix])
        computed.append({_("Gebäudekennungen"): building_names})
    if "address" in props:
        address = props["address"]
        computed.append({_("Adresse"): f"{address['street']}, {address['plz_place']}"})
    if "stats" in props:
        _append_if_present(props["stats"], computed, "n_buildings", _("Anzahl Gebäude"))
        _append_if_present(props["stats"], computed, "n_seats", _("Sitzplätze"))
        if "n_rooms" in props["stats"]:
            if props["stats"]["n_rooms"] == props["stats"]["n_rooms_reg"]:
                computed.append({_("Anzahl Räume"): str(props["stats"]["n_rooms"])})
            else:
                value = _("{n_rooms} ({n_rooms_reg} ohne Flure etc.)").format(
                    n_rooms=props["stats"]["n_rooms"],
                    n_rooms_reg=props["stats"]["n_rooms_reg"],
                )
                computed.append({_("Anzahl Räume"): value})
    if "generic" in props:
        for entity in props["generic"]:
            if isinstance(entity[1], dict):
                computed.append({"name": entity[0], **entity[1]})
            else:
                computed.append({"name": entity[0], "text": entity[1]})
    return computed


def localize_links(data):
    """
    Reformat the "links" value in "props" to be explicitly localized.
    This is a convenience function for the source data format that converts e.g.:
      `text: "<str>"`
    into
      `text: { de: "<str>", en: "<str>" }`
    """
    for _id, entry in data.items():
        links = entry.get("props", {}).get("links", None)
        if links:
            for link in links:
                if isinstance(link["text"], str):
                    link["text"] = {"de": link["text"], "en": link["text"]}
                if isinstance(link["url"], str):
                    link["url"] = {"de": link["url"], "en": link["url"]}


def generate_buildings_overview(data):
    """
    Generate the "buildings_overview" section
    """
    for _id, entry in data.items():
        if entry["type"] not in {"area", "site", "campus"} or "children_flat" not in entry:
            continue

        options = entry.get("generators", {}).get("buildings_overview", {"n_visible": 6, "list_start": []})

        # Collect buildings to display for this entry.
        buildings = []
        for child_id in entry["children"]:
            child = data[child_id]
            if child["type"] in {"area", "site", "campus", "building", "joined_building"}:
                buildings.append(child)
        # for child_id in entry["children_flat"]:
        #    child = data[child_id]
        #    if child["type"] == "joined_building" or \
        #       (child["type"] == "building"
        #        and data[child["parents"][-1]]["type"] != "joined_building"):
        #        buildings.append(child)
        # Entries are sorted alphabetically in second order to be predictable
        buildings = sorted(buildings, key=lambda e: (len(e.get("children_flat", [])), e["name"]), reverse=True)

        # The "list_start" can overwrite how the list of buildings starts,
        # and optionally also add other entries. All other entries are appended
        # after them.
        merged_ids = options["list_start"] + [b["id"] for b in buildings if b["id"] not in options["list_start"]]

        b_overview = entry.setdefault("sections", {}).setdefault("buildings_overview", {})
        b_overview["n_visible"] = options["n_visible"]
        b_overview["entries"] = []
        for child_id in merged_ids:
            try:
                child = data[child_id]
            except KeyError as err:
                raise RuntimeError(f"Unknown id '{child_id}' when generating buildings_overview for '{_id}'") from err

            n_rooms = child["props"]["stats"].get("n_rooms", 0)
            n_buildings = child["props"]["stats"].get("n_buildings", 0)
            if child["type"] in {"building", "joined_building"}:
                if n_rooms == 0:
                    subtext = _("Keine Räume bekannt")
                else:
                    subtext = _("{n_rooms} Räume").format(n_rooms=n_rooms)
            elif child["type"] == "area":
                subtext = _("{n_buildings} Gebäude, {n_rooms} Räume").format(n_buildings=n_buildings, n_rooms=n_rooms)
            elif child["type"] == "site":
                subtext = _("{n_buildings} Gebäude, {n_rooms} Räume (Außenstelle)").format(
                    n_buildings=n_buildings,
                    n_rooms=n_rooms,
                )
            else:
                raise RuntimeError(
                    f"Cannot generate buildings_overview subtext for type '{child['type']}', "
                    f"for: '{_id}', child id: '{child_id}'",
                )

            b_overview["entries"].append(
                {
                    "id": child_id,
                    "name": child["short_name"] if "short_name" in child else child["name"],
                    "subtext": subtext,
                    "thumb": child["imgs"][0]["name"] if child.get("imgs", []) else None,
                },
            )


def generate_rooms_overview(data):
    """
    Generate the "rooms_overview" section
    """
    for _id, entry in data.items():
        # if entry["type"] not in {"building", "joined_building", "virtual_room"} or \
        if (
            entry["type"] not in {"area", "site", "campus", "building", "joined_building", "virtual_room"}
            or "children_flat" not in entry
        ):
            continue

        rooms = {}
        for child_id in entry["children_flat"]:
            child = data[child_id]
            if child["type"] == "room":
                usage = child["usage"] if "usage" in child else {"name": _("Unbekannt")}
                rooms.setdefault(usage["name"], []).append(
                    {
                        "id": child_id,
                        "name": child["name"],
                    },
                )

        r_overview = entry.setdefault("sections", {}).setdefault("rooms_overview", {})
        r_overview["usages"] = [
            {
                "name": u[0],
                "count": len(u[1]),
                "children": sorted(u[1], key=lambda r: r["name"]),
            }
            for u in sorted(rooms.items(), key=lambda e: e[0])
        ]
