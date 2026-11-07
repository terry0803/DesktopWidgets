"""Draws the icons and writes app.res: the program's icon, the tray icons, and its name for Windows.

Run with Python and Pillow after changing anything here. build.rs links the result into the program.
"""
import io, struct
from pathlib import Path
from PIL import Image, ImageDraw

HERE = Path(__file__).parent
NAME = "桌面小工具"
VERSION = (1, 0, 0, 0)


def glyph(size, tiles, back=None, outline=False):
    """Widgets: one wide tile on top, two below, like the cards on the desktop. The tray's is drawn in thin lines,
    like the other icons there; the program's own is filled, on a dark tile."""
    scale = 4
    big = size * scale
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    if back:
        draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=big * 0.22, fill=back)
        inset = big * 0.2
    else:
        inset = big * (0.08 if outline else 0.06)
    span = big - inset * 2
    gap = span * (0.14 if outline else 0.12)
    half = (span - gap) / 2
    radius = half * (0.32 if outline else 0.28)
    # One pixel at 100%, so it matches the line icons beside it at any scale.
    stroke = max(scale, round(size / 16 * scale)) if outline else 0
    x0, y0 = inset, inset
    boxes = [
        (x0, y0, x0 + span, y0 + half),
        (x0, y0 + half + gap, x0 + half, y0 + span),
        (x0 + half + gap, y0 + half + gap, x0 + span, y0 + span),
    ]
    for box in boxes:
        if outline:
            draw.rounded_rectangle(box, radius=radius, outline=tiles, width=stroke)
        else:
            draw.rounded_rectangle(box, radius=radius, fill=tiles)
    return image.resize((size, size), Image.LANCZOS)


def ico_images(make, sizes):
    """Each size as icon image data: a DIB for the small ones, PNG for 256, as Windows expects."""
    out = []
    for size in sizes:
        image = make(size)
        if size >= 256:
            data = io.BytesIO()
            image.save(data, "PNG")
            out.append((size, data.getvalue()))
            continue
        pixels = image.tobytes("raw", "BGRA")
        rows = [pixels[y * size * 4:(y + 1) * size * 4] for y in range(size)]
        xor = b"".join(reversed(rows))
        mask_row = ((size + 31) // 32) * 4
        mask = b"\0" * (mask_row * size)
        header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(xor) + len(mask), 0, 0, 0, 0)
        out.append((size, header + xor + mask))
    return out


entries = []


def resource(kind, name, data, language=0x0404):
    """One entry of a .res file, numeric type and name."""
    if kind:
        entries.append((kind, name, language, data))
    header = struct.pack("<II", len(data), 0)
    header += struct.pack("<HH", 0xFFFF, kind) + struct.pack("<HH", 0xFFFF, name)
    header = header[:4] + struct.pack("<I", 8 + 4 + 4 + 16) + header[8:]
    header += struct.pack("<IHHII", 0, 0x1030, language, 0, 0)
    entry = header + data
    entry += b"\0" * (-len(entry) % 4)
    return entry


def version_block(key, value=None, children=(), text=True):
    key_bytes = (key + "\0").encode("utf-16-le")
    body = struct.pack("<HHH", 0, 0, 1 if text else 0) + key_bytes
    body += b"\0" * (-len(body) % 4)
    if value is not None:
        if text:
            value_bytes = (value + "\0").encode("utf-16-le")
            length = len(value_bytes) // 2
        else:
            value_bytes = value
            length = len(value_bytes)
        body = body[:2] + struct.pack("<H", length) + body[4:]
        body += value_bytes
        body += b"\0" * (-len(body) % 4)
    for child in children:
        body += child
        body += b"\0" * (-len(body) % 4)
    return struct.pack("<H", len(body)) + body[2:]


def version():
    major, minor, patch, build = VERSION
    fixed = struct.pack("<IIIIIIIIIIIII", 0xFEEF04BD, 0x00010000, (major << 16) | minor, (patch << 16) | build,
                        (major << 16) | minor, (patch << 16) | build, 0x3F, 0, 0x40004, 1, 0, 0, 0)
    dotted = ".".join(map(str, VERSION))
    strings = [version_block(k, v) for k, v in [
        ("FileDescription", NAME), ("ProductName", NAME), ("FileVersion", dotted), ("ProductVersion", dotted),
        ("InternalName", "desktop-widgets"), ("OriginalFilename", "desktop-widgets.exe"), ("CompanyName", NAME),
    ]]
    table = version_block("040404b0", None, strings)
    string_info = version_block("StringFileInfo", None, [table])
    var = version_block("Translation", struct.pack("<HH", 0x0404, 0x04B0), text=False)
    var_info = version_block("VarFileInfo", None, [var])
    return version_block("VS_VERSION_INFO", fixed, [string_info, var_info], text=False)


def coff(resources):
    """What windres makes of a .res file: an object with a .rsrc section, which GNU ld links into the program."""
    by_type = {}
    for kind, name, language, data in resources:
        by_type.setdefault(kind, {})[name] = (language, data)
    kinds = sorted(by_type)
    # Layout: the root directory, a directory per type, one per name, then the data entries, then the data.
    offset = 16 + 8 * len(kinds)
    type_dirs = {}
    for kind in kinds:
        type_dirs[kind] = offset
        offset += 16 + 8 * len(by_type[kind])
    name_dirs = {}
    for kind in kinds:
        for name in sorted(by_type[kind]):
            name_dirs[(kind, name)] = offset
            offset += 16 + 8
    data_entries = {}
    for kind in kinds:
        for name in sorted(by_type[kind]):
            data_entries[(kind, name)] = offset
            offset += 16
    blobs = {}
    for kind in kinds:
        for name in sorted(by_type[kind]):
            offset += -offset % 8
            blobs[(kind, name)] = offset
            offset += len(by_type[kind][name][1])
    section = bytearray(offset + (-offset % 8))
    directory = lambda count: struct.pack("<IIHHHH", 0, 0, 0, 0, 0, count)
    section[0:16] = directory(len(kinds))
    for index, kind in enumerate(kinds):
        section[16 + 8 * index:24 + 8 * index] = struct.pack("<II", kind, 0x80000000 | type_dirs[kind])
    relocations = []
    for kind in kinds:
        names = sorted(by_type[kind])
        base = type_dirs[kind]
        section[base:base + 16] = directory(len(names))
        for index, name in enumerate(names):
            at = base + 16 + 8 * index
            section[at:at + 8] = struct.pack("<II", name, 0x80000000 | name_dirs[(kind, name)])
            language, data = by_type[kind][name]
            sub = name_dirs[(kind, name)]
            section[sub:sub + 16] = directory(1)
            section[sub + 16:sub + 24] = struct.pack("<II", language, data_entries[(kind, name)])
            entry = data_entries[(kind, name)]
            section[entry:entry + 16] = struct.pack("<IIII", blobs[(kind, name)], len(data), 0, 0)
            relocations.append(entry)  # Holds the data's offset; the linker adds where the section ends up.
            start = blobs[(kind, name)]
            section[start:start + len(data)] = data
    header_size = 20 + 40
    raw_at = header_size
    relocations_at = raw_at + len(section)
    symbols_at = relocations_at + 10 * len(relocations)
    out = struct.pack("<HHIIIHH", 0x8664, 1, 0, symbols_at, 1, 0, 0)
    out += struct.pack("<8sIIIIIIHHI", b".rsrc", 0, 0, len(section), raw_at, relocations_at, 0, len(relocations), 0, 0x40000040)
    out += bytes(section)
    for at in relocations:
        out += struct.pack("<IIH", at, 0, 3)  # IMAGE_REL_AMD64_ADDR32NB against the section's own symbol.
    out += struct.pack("<8sIhHBB", b".rsrc", 0, 1, 0, 3, 0)
    out += struct.pack("<I", 4)
    return out


def main():
    sets = {
        1: (lambda s: glyph(s, (245, 245, 247, 255), back=(32, 32, 36, 255)), [16, 20, 24, 32, 40, 48, 64, 256]),
        2: (lambda s: glyph(s, (255, 255, 255, 255), outline=True), [16, 20, 24, 32, 40, 48]),  # Tray, dark taskbar.
        3: (lambda s: glyph(s, (24, 24, 26, 255), outline=True), [16, 20, 24, 32, 40, 48]),  # Tray, light taskbar.
    }
    res = resource(0, 0, b"")[:0]
    # A .res file starts with an empty entry.
    res = struct.pack("<IIHHHHIHHII", 0, 32, 0xFFFF, 0, 0xFFFF, 0, 0, 0, 0, 0, 0)
    next_id = 1
    for group, (make, sizes) in sets.items():
        directory = struct.pack("<HHH", 0, 1, len(sizes))
        for size, data in ico_images(make, sizes):
            res += resource(3, next_id, data)
            side = 0 if size >= 256 else size
            directory += struct.pack("<BBBBHHIH", side, side, 0, 0, 1, 32, len(data), next_id)
            next_id += 1
        res += resource(14, group, directory)
    res += resource(16, 1, version())
    (HERE / "app.res").write_bytes(res)
    (HERE / "app.o").write_bytes(coff(entries))
    make, _ = sets[1]
    make(256).save(HERE / "app.ico", sizes=[(16, 16), (32, 32), (48, 48), (256, 256)])
    print("wrote", HERE / "app.res", len(res), "bytes")


if __name__ == "__main__":
    main()
