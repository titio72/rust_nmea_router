#!/usr/bin/env python3
"""Print one PGN's canboat definition as an implementation spec for the nmea2k crate.

The definition comes from `keel emit` (canboat's PGN database tool), so it carries what
pgns.json lacks or gets wrong: exact bit offsets, the field's sentinel values, range limits,
lookup enumerations and the canboat sample frames to use as test fixtures.

usage: python3 pgn_spec.py <PGN> [--canboat DIR]

The canboat checkout is found via --canboat, then $CANBOAT_DIR, then ~/dev/canboat.
"""

import argparse
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]  # .claude/skills/add-pgn/ -> repo root


def run_keel_emit(canboat: Path) -> str:
    keel = canboat / "target" / "release" / "keel"
    cmd = [str(keel), "emit"] if keel.exists() else ["cargo", "run", "-q", "--release", "-p", "keel", "--", "emit"]
    try:
        return subprocess.run(cmd, cwd=canboat, check=True, capture_output=True, text=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        sys.exit(f"keel emit failed in {canboat}: {e}")


def text(el, tag, default=None):
    child = el.find(tag)
    return child.text if child is not None and child.text is not None else default


def struct_name(description: str) -> str:
    # Acronyms are title-cased, as in the crate: AisClassAPositionReport, GnssDops
    words = re.split(r"[^A-Za-z0-9]+", re.sub(r"\(.*?\)", "", description))
    return "".join((w.capitalize() if w.isupper() and len(w) > 1 else w[:1].upper() + w[1:])
                   for w in words if w)


def float_lit(v: str) -> str:
    """A Rust f64 literal: `100` would not multiply an f64."""
    return v if any(c in v for c in ".eE") else f"{v}.0"


def snake(identifier: str) -> str:
    return re.sub(r"(?<=[a-z0-9])([A-Z])", r"_\1", identifier).lower()


def int_type(bits: int, signed: bool) -> str:
    for size in (8, 16, 32, 64):
        if bits <= size:
            return f"{'i' if signed else 'u'}{size}"
    return "u64"


def raw_read(off: int, bits: int, signed: bool) -> tuple[str, bool]:
    """Rust expression for the raw value, and whether it is a full-width native integer."""
    ty = int_type(bits, signed)
    if off % 8 == 0 and bits in (8, 16, 32, 64):
        first = off // 8
        if bits == 8:
            return (f"(data[{first}] as i8)" if signed else f"data[{first}]"), True
        idx = ", ".join(f"data[{first + i}]" for i in range(bits // 8))
        return f"{ty}::from_le_bytes([{idx}])", True
    fn = "read_signed_bits" if signed else "read_bits"
    return f"({fn}(data, {off}, {bits}) as {ty})", False


def fmt_num(v):
    if v is None:
        return None
    f = float(v)
    return str(int(f)) if f.is_integer() else repr(f)


def rust_expr(f: dict) -> str:
    kind, off, bits = f["kind"], f["off"], f["bits"]
    if kind in ("RESERVED", "SPARE"):
        return "— skip"
    if off is None or bits is None:
        return "— variable position: read manually after the preceding variable-length field"
    if kind.startswith("STRING_FIX"):
        return f"extract_text_from_bytes(data, {off // 8}, {bits // 8})"
    if kind.startswith("STRING"):
        return "— variable-length string (length/encoding prefix): read manually"
    if kind in ("BINARY", "VARIABLE", "DYNAMIC_FIELD_VALUE"):
        return "— binary/variable: decide per use, usually not decoded"

    raw, native = raw_read(off, bits, f["signed"])
    has_sentinels = f["unknown"] is not None
    if has_sentinels and native:
        raw = f"{raw}.available()"
    elif has_sentinels:
        lowest = min(int(v) for v in (f["unknown"], f["oor"], f["reserved"]) if v is not None)
        raw = f"Some({raw}).filter(|&r| r < {lowest})" if not f["signed"] else \
            f"{raw} /* signed {bits}-bit: N/A is {f['unknown']} as raw pattern, check manually */"

    res = float_lit(f["res"]) if f["res"] is not None else None
    scaled = res is not None and float(res) != 1.0
    if kind in ("LOOKUP", "INDIRECT_LOOKUP", "BITLOOKUP"):
        expr = raw if has_sentinels else raw  # map the code onto the enum in the decoder
        return f"{expr}  → enum {f['lookup'] or '?'}"
    if kind == "TIME":
        expr = f"{raw}.map(|r| r as f64 * {res})" if has_sentinels else f"{raw} as f64 * {res}"
        return f"{expr}  → seconds since midnight (N2kDateTime.time)"
    if kind == "DATE":
        return f"{raw}  → days since 1970-01-01 (N2kDateTime.date)"
    if scaled:
        expr = f"{raw}.map(|r| r as f64 * {res})" if has_sentinels else f"{raw} as f64 * {res}"
    else:
        expr = raw
    name = f["id"].lower()
    if name.startswith("latitude") or name.endswith("latitude"):
        expr = f"latitude({expr})"
    elif name.startswith("longitude") or name.endswith("longitude"):
        expr = f"longitude({expr})"
    return expr


def parse_pgns(xml: str, pgn: int):
    root = ET.fromstring(xml)
    lookups = {}
    for section, tag in (("LookupEnumerations", "LookupEnumeration"),
                         ("LookupBitEnumerations", "LookupBitEnumeration"),
                         ("LookupIndirectEnumerations", "LookupIndirectEnumeration")):
        for le in root.iter(tag):
            pairs = [(p.get("Value") or p.get("Bit") or p.get("Value1"), p.get("Name"))
                     for p in le if p.get("Name")]
            lookups[le.get("Name")] = pairs

    variants = []
    for info in root.iter("PGNInfo"):
        if text(info, "PGN") != str(pgn):
            continue
        fields = []
        for fe in info.iter("Field"):
            fields.append({
                "order": text(fe, "Order"),
                "id": text(fe, "Id", ""),
                "name": text(fe, "Name", ""),
                "off": int(text(fe, "BitOffset")) if text(fe, "BitOffset") else None,
                "bits": int(text(fe, "BitLength")) if text(fe, "BitLength") else None,
                "signed": text(fe, "Signed") == "true",
                "res": text(fe, "Resolution"),
                "unit": text(fe, "Unit", ""),
                "kind": text(fe, "FieldType", "?"),
                "min": fmt_num(text(fe, "RangeMin")),
                "max": fmt_num(text(fe, "RangeMax")),
                "unknown": text(fe, "UnknownValue"),
                "oor": text(fe, "OutOfRangeValue"),
                "reserved": text(fe, "ReservedValue"),
                "lookup": text(fe, "LookupEnumeration") or text(fe, "LookupBitEnumeration")
                          or text(fe, "LookupIndirectEnumeration"),
                "match": text(fe, "Match"),
            })
        variants.append({
            "id": text(info, "Id", ""),
            "description": text(info, "Description", ""),
            "type": text(info, "Type", "?"),
            "length": text(info, "Length"),
            "priority": text(info, "Priority"),
            "interval": text(info, "TransmissionInterval"),
            "repeating": [(text(info, f"RepeatingFieldSet{n}Size"), text(info, f"RepeatingFieldSet{n}StartField"))
                          for n in (1, 2) if text(info, f"RepeatingFieldSet{n}Size")],
            "fields": fields,
        })
    return variants, lookups


def samples(canboat: Path, pgn: int, variant_id: str, limit: int = 4):
    path = canboat / "database" / "pgns" / f"{pgn:06d}-{variant_id}.yaml"
    if not path.exists():
        return path, []
    lines = path.read_text().splitlines()
    try:
        start = lines.index("samples:") + 1
    except ValueError:
        return path, []
    out, current = [], None
    for line in lines[start:]:
        if line.startswith("- raw:"):
            if len(out) == limit:
                break
            raw = line.split(":", 1)[1].split("#")[0].strip().strip('"')
            parts = raw.split(",")
            current = {"raw": raw, "data": ",".join(parts[6:]), "expects": []}
            out.append(current)
        elif current is not None and line.startswith("    ") and not line.strip().startswith("expects"):
            current["expects"].append(line.strip())
        elif line and not line.startswith(" ") and not line.startswith("-"):
            break
    return path, out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pgn", type=int)
    ap.add_argument("--canboat", type=Path,
                    default=Path(os.environ.get("CANBOAT_DIR", Path.home() / "dev" / "canboat")))
    args = ap.parse_args()

    variants, lookups = parse_pgns(run_keel_emit(args.canboat), args.pgn)
    if not variants:
        sys.exit(f"PGN {args.pgn} is not in the canboat database")

    existing = REPO / "nmea2k" / "src" / "pgns" / f"pgn{args.pgn}.rs"
    if existing.exists():
        print(f"NOTE: {existing.relative_to(REPO)} already exists — extend it, do not recreate it.\n")
    if len(variants) > 1:
        print(f"NOTE: {len(variants)} variants (proprietary/manufacturer-specific): "
              "pick the one matching the device, using the Match column.\n")

    for v in variants:
        fast = v["type"] in ("Fast", "Mixed")
        print(f"PGN {args.pgn}  id={v['id']}  \"{v['description']}\"")
        print(f"  struct {struct_name(v['description'])}   file nmea2k/src/pgns/pgn{args.pgn}.rs")
        print(f"  type {v['type']}{'  → FAST PACKET: register in stream_reader.rs is_fast_packet_pgn' if fast else ''}")
        print(f"  length {v['length'] or 'variable'} bytes   priority {v['priority']}   interval {v['interval'] or '-'} ms")
        for size, start in v["repeating"]:
            print(f"  repeating set: {size} fields from field #{start} — decode into a Vec")
        print("  fields:")
        for f in v["fields"]:
            rng = f"{f['min']}..{f['max']}" if f["min"] is not None else "-"
            na = "/".join(x for x in (f["unknown"], f["oor"], f["reserved"]) if x) or "none"
            print(f"   #{f['order']} {f['id']} ({snake(f['id'])}) \"{f['name']}\"  {f['kind']}"
                  f"{' MATCH=' + f['match'] if f['match'] else ''}")
            print(f"       bits {f['off']}+{f['bits']}  {'signed' if f['signed'] else 'unsigned'}"
                  f"  res {f['res']} {f['unit']}  range {rng}  N/A/OOR/RES raw {na}"
                  f"{'  lookup ' + f['lookup'] if f['lookup'] else ''}")
            print(f"       rust: {rust_expr(f)}")
        used = sorted({f["lookup"] for f in v["fields"] if f["lookup"]})
        for name in used:
            pairs = lookups.get(name, [])
            print(f"  lookup {name}: " + ", ".join(f"{k}={n}" for k, n in pairs))
        path, smp = samples(args.canboat, args.pgn, v["id"])
        print(f"  samples ({path.relative_to(args.canboat)}):" if smp else
              f"  samples: none in {path.relative_to(args.canboat)} — build test frames by hand")
        for s in smp:
            print(f"   data [{s['data']}]")
            for e in s["expects"]:
                print(f"     {e}")
        print()


if __name__ == "__main__":
    main()
