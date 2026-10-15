"""Map EventFlags, evaluated from the effective DB4 records (also offline).

Matches local_map_state.event_set_enabled in the server: positive flags are
alternatives; any active negative flag vetoes the set. Id is the reference,
not SQLite rowid or the order in which the records happen to be returned.
"""
from level_data import GEOMETRY


def declarations(text):
    pairs = []
    for token in str(text or "").split(";"):
        name, separator, value = token.partition("=")
        if separator and name.strip():
            pairs.append((name.strip().casefold(), value.strip().casefold() == "true"))
    return pairs


def enabled(text, active):
    active = {flag.casefold() for flag in active}
    pairs = declarations(text)
    positive = {name for name, yes in pairs if yes}
    negative = {name for name, yes in pairs if not yes}
    return (not positive or bool(positive & active)) and not (negative & active)


class MapEvents:
    def __init__(self, records, geometry_rules=None):
        self.geometry_rules = geometry_rules or {}
        self.sets = {}
        for row in records.get("_Instance_EventSets", {}).values():
            try:
                self.sets[int(row["Id"])] = row.get("EventFlags") or ""
            except (KeyError, TypeError, ValueError):
                continue
        self.referenced = {self.reference(kind, row) for kind, rows in records.items() for row in rows.values()}
        self.flags = sorted({flag for key in self.referenced for flag, _ in declarations(self.sets.get(key))})
        self.flags = sorted(set(self.flags) | {row['event'].casefold() for row in records.get(GEOMETRY, {}).values() if row.get('event')}
                            | {flag for rule in self.geometry_rules.values() for flag, _ in declarations(rule)})

    def preview_flags(self, flag=None):
        active = {flag.casefold()} if flag else set()
        # Authored winter scenery is a separate silent event, used alongside
        # the festival. This is an editor preset, not a server scheduling rule.
        if 'christmas_2012' in active and 'christmas_2012_silent' in self.flags:
            active.add('christmas_2012_silent')
        return active

    @staticmethod
    def reference(kind, row):
        # Geometry's `g` is a placement group, not an EventSet index.
        return -1 if kind == GEOMETRY else int(row.get("EventSetRow") if row.get("EventSetRow") is not None else -1)

    def active_sets(self, flag=None):
        active = [flag] if isinstance(flag, str) else (flag or [])
        return {-1, *(key for key, text in self.sets.items() if enabled(text, active))}

    def visible(self, kind, row, flag=None, ident=None):
        if kind == GEOMETRY:
            active = [flag] if isinstance(flag, str) else (flag or [])
            return enabled(self.describe(kind, row, ident), active)
        return self.reference(kind, row) in self.active_sets(flag)

    def describe(self, kind, row, ident=None):
        if kind == GEOMETRY:
            return (row['event'] + '=True;') if row.get('event') else self.geometry_rules.get(ident, 'Always visible')
        key = self.reference(kind, row)
        return "Always visible" if key == -1 else self.sets.get(key, f"Unknown event set {key}")


def event_label(flag):
    aliases = {"anniversary_event": "Anniversary", "christmas_2012": "Winter festival",
               "christmas_2012_silent": "Winter scenery only", "halloween_event_01": "Halloween",
               "easter_event_01": "Spring festival", "summer_festival_event_01": "Summer festival"}
    return f"{aliases.get(flag, flag.replace('_', ' ').title())}  ·  {flag}"
