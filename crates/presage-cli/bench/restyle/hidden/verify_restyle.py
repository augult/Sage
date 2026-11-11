"""Scores the restyled settings page on properties any careful redesign has.

Eleven checks, one point each, read from index.html and styles.css with a small CSS cascade
(no browser). Solved: the hooks app.js relies on are intact and at least NEED checks pass.
Nothing here asks for particular class names, colours or layout.
"""

import re
import sys
from html.parser import HTMLParser

NEED = 9
MAX_SPACINGS = 8
MAX_FONT_SIZES = 6
REQUIRED_IDS = {
    "settings-form": "form",
    "display-name": "input",
    "email": "input",
    "timezone": "select",
    "notify": "input",
    "bio": "textarea",
    "save": None,
    "reset": None,
    "status": None,
    "members": None,
    "member-filter": "input",
    "member-count": None,
    "invite-open": None,
    "invite-dialog": None,
    "invite-email": "input",
    "invite-send": None,
    "invite-cancel": None,
}
MEMBERS = ["Ines Vidal", "Tomas Reyes", "Mira Okafor", "Jonas Lind"]
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}
CONTROLS = {"button", "input", "select", "textarea"}

# ---------------------------------------------------------------------------------------------
# HTML


class Node:
    def __init__(self, tag, attrs, parent):
        self.tag, self.attrs, self.parent = tag, attrs, parent
        self.children, self.text = [], ""

    def ancestors(self):
        node = self.parent
        while node is not None:
            yield node
            node = node.parent

    def walk(self):
        yield self
        for child in self.children:
            yield from child.walk()

    def all_text(self):
        return self.text + "".join(child.all_text() for child in self.children)


class Tree(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = Node("#document", {}, None)
        self.current = self.root

    def handle_starttag(self, tag, attrs):
        node = Node(tag, {name: (value if value is not None else "") for name, value in attrs}, self.current)
        self.current.children.append(node)
        if tag not in VOID:
            self.current = node

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.current = self.current.parent

    def handle_endtag(self, tag):
        node = self.current
        while node is not None and node.tag != tag:
            node = node.parent
        if node is not None and node.parent is not None:
            self.current = node.parent

    def handle_data(self, data):
        if self.current.tag not in ("script", "style"):
            self.current.text += data


# ---------------------------------------------------------------------------------------------
# CSS


class Rule:
    def __init__(self, selector, declarations, media, order):
        self.selector, self.declarations, self.media, self.order = selector, declarations, media, order


def split_top(text, separator):
    """Splits on the separator outside brackets and quotes."""
    parts, depth, quote, start = [], 0, None, 0
    for index, char in enumerate(text):
        if quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char in "([":
            depth += 1
        elif char in ")]":
            depth -= 1
        elif char == separator and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return [part.strip() for part in parts if part.strip()]


def parse_declarations(text):
    declarations = {}
    for part in split_top(text, ";"):
        name, colon, value = part.partition(":")
        if colon:
            value = value.strip()
            important = value.lower().endswith("!important")
            if important:
                value = value[: -len("!important")].strip()
            declarations[name.strip().lower() if not name.strip().startswith("--") else name.strip()] = (value, important)
    return declarations


def parse_css(text):
    """The style rules of a sheet, flattened: nested rules and conditional groups are unfolded."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    rules = []

    def block(body, parents, media):
        index, own = 0, []
        while index < len(body):
            brace, semi = body.find("{", index), body.find(";", index)
            if brace == -1 or (semi != -1 and semi < brace):
                end = len(body) if semi == -1 else semi
                own.append(body[index:end])
                index = end + 1
                continue
            prelude = body[index:brace].strip()
            depth, close = 1, brace + 1
            while close < len(body) and depth:
                depth += {"{": 1, "}": -1}.get(body[close], 0)
                close += 1
            inner = body[brace + 1 : close - 1]
            index = close
            lowered = prelude.lower()
            if lowered.startswith(("@media", "@container")):
                block(inner, parents, (media + " and " if media else "") + prelude)
            elif lowered.startswith(("@supports", "@layer", "@scope")):
                block(inner, parents, media)
            elif lowered.startswith("@"):
                continue
            else:
                selectors = []
                for selector in split_top(prelude, ","):
                    for parent in parents or [""]:
                        if "&" in selector:
                            selectors.append(selector.replace("&", parent))
                        else:
                            selectors.append((parent + " " + selector).strip())
                block(inner, selectors, media)
        declarations = parse_declarations(";".join(own))
        if declarations and parents:
            for selector in parents:
                rules.append(Rule(selector, declarations, media, len(rules)))

    block(text, [], None)
    return rules


def resolve_vars(value, variables, depth=0):
    while "var(" in value and depth < 12:
        start = value.find("var(")
        level, end = 1, start + 4
        while end < len(value) and level:
            level += {"(": 1, ")": -1}.get(value[end], 0)
            end += 1
        name, _, fallback = value[start + 4 : end - 1].partition(",")
        replacement = variables.get(name.strip(), fallback.strip())
        value = value[:start] + replacement + value[end:]
        depth += 1
    return value.strip()


# Selectors.

STATE_PSEUDOS = {"hover", "focus", "focus-visible", "focus-within", "active", "visited", "target"}
COMPOUND = re.compile(
    r"(?P<tag>^[a-zA-Z][\w-]*|^\*)|#(?P<id>[\w-]+)|\.(?P<cls>[\w-]+)"
    r"|\[(?P<attr>[\w-]+)\s*(?:(?P<op>[~|^$*]?=)\s*[\"']?(?P<val>[^\"'\]]*)[\"']?\s*[is]?)?\]"
    r"|(?P<pe>::[\w-]+)|:(?P<pseudo>[\w-]+)(?:\((?P<arg>(?:[^()]|\([^()]*\))*)\))?"
)


def parse_selector(selector):
    """[(combinator, [parts])] left to right, or None when the selector targets a pseudo-element."""
    tokens = re.findall(r"\s*[>+~]\s*|\s+|(?:[^\s>+~()\[\]]|\((?:[^()]|\([^()]*\))*\)|\[[^\]]*\])+", selector.strip())
    sequence, combinator = [], " "
    for token in tokens:
        if not token.strip() or token.strip() in ">+~":
            combinator = token.strip() or " "
            continue
        parts, position = [], 0
        while position < len(token):
            match = COMPOUND.match(token, position) if position else COMPOUND.match(token)
            if match is None or match.end() == position:
                match = re.compile(COMPOUND.pattern.replace("^", "")).match(token, position)
                if match is None or match.end() == position:
                    return None
            if match.group("pe") or match.group("pseudo") in ("before", "after", "first-line", "first-letter", "placeholder"):
                return None
            parts.append(match.groupdict())
            position = match.end()
        sequence.append((combinator, parts))
        combinator = " "
    return sequence or None


def nth(argument, position):
    argument = argument.replace(" ", "").lower().split("of")[0]
    if argument == "odd":
        return position % 2 == 1
    if argument == "even":
        return position % 2 == 0
    match = re.fullmatch(r"([+-]?\d*)n([+-]\d+)?", argument)
    if match:
        step = int(match.group(1) + "1") if match.group(1) in ("", "+", "-") else int(match.group(1))
        offset = int(match.group(2) or 0)
        return step == 0 and position == offset or step != 0 and (position - offset) % step == 0 and (position - offset) // step >= 0
    return argument.lstrip("+").isdigit() and position == int(argument)


def matches_compound(node, parts, state):
    for part in parts:
        if part["tag"] and part["tag"] != "*" and part["tag"].lower() != node.tag:
            return False
        if part["id"] and node.attrs.get("id") != part["id"]:
            return False
        if part["cls"] and part["cls"] not in node.attrs.get("class", "").split():
            return False
        if part["attr"]:
            name, op, wanted = part["attr"].lower(), part["op"], part["val"] or ""
            value = node.attrs.get(name)
            if "disabled" in state and node.tag in CONTROLS:
                value = {"disabled": "", "aria-disabled": "true"}.get(name, value)
            if value is None:
                return False
            if op and not {
                "=": value == wanted,
                "~=": wanted in value.split(),
                "|=": value == wanted or value.startswith(wanted + "-"),
                "^=": value.startswith(wanted),
                "$=": value.endswith(wanted),
                "*=": wanted in value,
            }[op]:
                return False
        pseudo = part["pseudo"]
        if not pseudo:
            continue
        pseudo, argument = pseudo.lower(), part["arg"] or ""
        siblings = [child for child in node.parent.children] if node.parent else [node]
        alike = [child for child in siblings if child.tag == node.tag]
        if pseudo in STATE_PSEUDOS:
            ok = pseudo in state
        elif pseudo == "disabled":
            ok = node.tag in CONTROLS and ("disabled" in node.attrs or "disabled" in state)
        elif pseudo == "enabled":
            ok = node.tag in CONTROLS and "disabled" not in node.attrs and "disabled" not in state
        elif pseudo == "checked":
            ok = "checked" in node.attrs or "selected" in node.attrs
        elif pseudo in ("link", "any-link"):
            ok = node.tag == "a" and "href" in node.attrs
        elif pseudo == "root":
            ok = node.tag == "html"
        elif pseudo == "first-child":
            ok = siblings[0] is node
        elif pseudo == "last-child":
            ok = siblings[-1] is node
        elif pseudo == "only-child":
            ok = len(siblings) == 1
        elif pseudo == "first-of-type":
            ok = alike[0] is node
        elif pseudo == "last-of-type":
            ok = alike[-1] is node
        elif pseudo == "nth-child":
            ok = nth(argument, siblings.index(node) + 1)
        elif pseudo == "nth-last-child":
            ok = nth(argument, len(siblings) - siblings.index(node))
        elif pseudo == "nth-of-type":
            ok = nth(argument, alike.index(node) + 1)
        elif pseudo == "empty":
            ok = not node.children and not node.text.strip()
        elif pseudo in ("is", "where", "matches", "any"):
            ok = any(matches(node, option, state) for option in split_top(argument, ","))
        elif pseudo == "not":
            ok = not any(matches(node, option, state) for option in split_top(argument, ","))
        elif pseudo in ("modal", "open", "popover-open"):
            ok = node.tag == "dialog"
        elif pseudo in ("placeholder-shown", "invalid", "user-invalid", "indeterminate", "has", "required", "optional", "valid"):
            ok = False
        else:
            ok = False
        if not ok:
            return False
    return True


PARSED = {}


def matches(node, selector, state=frozenset()):
    if selector not in PARSED:
        PARSED[selector] = parse_selector(selector)
    sequence = PARSED[selector]
    if sequence is None:
        return False

    def step(node, index):
        combinator, parts = sequence[index]
        if not matches_compound(node, parts, state):
            return False
        if index == 0:
            return True
        if combinator == ">":
            return node.parent is not None and step(node.parent, index - 1)
        if combinator in "+~":
            siblings = node.parent.children if node.parent else []
            before = siblings[: siblings.index(node)] if node in siblings else []
            candidates = before[-1:] if combinator == "+" else before
            return any(step(sibling, index - 1) for sibling in candidates)
        return any(step(ancestor, index - 1) for ancestor in node.ancestors())

    return step(node, len(sequence) - 1)


def specificity(selector):
    selector = re.sub(r":where\((?:[^()]|\([^()]*\))*\)", "", selector)
    ids = len(re.findall(r"#[\w-]+", selector))
    classes = len(re.findall(r"\.[\w-]+|\[[^\]]*\]|(?<!:):(?!is\(|not\(|where\()[\w-]+", selector))
    tags = len(re.findall(r"(?:^|[\s>+~(,])[a-zA-Z][\w-]*", selector))
    return ids, classes, tags


class Sheet:
    def __init__(self, text):
        self.rules = parse_css(text)
        self.variables = {}
        for rule in self.rules:
            if rule.media is None or "prefers-color-scheme" not in rule.media:
                for name, (value, _) in rule.declarations.items():
                    if name.startswith("--") and (rule.media is None or name not in self.variables):
                        self.variables[name] = value
        self.cache = {}

    def value(self, text):
        return resolve_vars(text, self.variables)

    def declared(self, node, state=frozenset()):
        """property -> value for the node: the winning declarations of the rules outside @media."""
        key = (id(node), state)
        if key not in self.cache:
            winners = {}
            for rule in self.rules:
                if rule.media is not None or not matches(node, rule.selector, state):
                    continue
                for name, (value, important) in rule.declarations.items():
                    rank = (important, specificity(rule.selector), rule.order)
                    for longhand, part in expand(name, self.value(value)):
                        if longhand not in winners or rank >= winners[longhand][0]:
                            winners[longhand] = (rank, part)
            self.cache[key] = {name: value for name, (_, value) in winners.items()}
        return self.cache[key]


def expand(name, value):
    """Shorthands the checks care about, as longhands."""
    if name in ("padding", "margin"):
        parts = split_top(value, " ") or [""]
        top, right, bottom, left = (parts * 4)[:4] if len(parts) == 1 else (parts * 2)[:4] if len(parts) == 2 else (parts + [parts[1]])[:4]
        return [(f"{name}-top", top), (f"{name}-right", right), (f"{name}-bottom", bottom), (f"{name}-left", left)]
    if name in ("padding-inline", "padding-block", "margin-inline", "margin-block"):
        box, axis = name.split("-")
        parts = split_top(value, " ") or [""]
        sides = ("left", "right") if axis == "inline" else ("top", "bottom")
        return [(f"{box}-{sides[0]}", parts[0]), (f"{box}-{sides[1]}", parts[-1])]
    if name == "background":
        colour = next((part for part in split_top(value, " ") if parse_colour(part) is not None), None)
        if colour is None and ("gradient(" in value or "url(" in value):
            return [("background-color", "?")]
        return [("background-color", colour or "transparent")]
    if name == "font":
        size = next((part.split("/")[0] for part in split_top(value, " ") if re.match(r"^[\d.]+(px|rem|em|%|pt)|^(clamp|calc|min|max)\(", part)), None)
        return [("font", value)] + ([("font-size", size)] if size else [("font-size", "inherit")] if value.strip() == "inherit" else [])
    return [(name, value)]


# Colours.

NAMED = {
    "black": "#000000", "white": "#ffffff", "red": "#ff0000", "green": "#008000", "blue": "#0000ff",
    "gray": "#808080", "grey": "#808080", "silver": "#c0c0c0", "navy": "#000080", "teal": "#008080",
    "maroon": "#800000", "purple": "#800080", "olive": "#808000", "orange": "#ffa500", "yellow": "#ffff00",
    "lime": "#00ff00", "aqua": "#00ffff", "cyan": "#00ffff", "fuchsia": "#ff00ff", "magenta": "#ff00ff",
    "whitesmoke": "#f5f5f5", "gainsboro": "#dcdcdc", "lightgray": "#d3d3d3", "lightgrey": "#d3d3d3",
    "darkgray": "#a9a9a9", "darkgrey": "#a9a9a9", "dimgray": "#696969", "dimgrey": "#696969",
    "slategray": "#708090", "slategrey": "#708090", "darkslategray": "#2f4f4f", "darkslategrey": "#2f4f4f",
    "lightslategray": "#778899", "ghostwhite": "#f8f8ff", "snow": "#fffafa", "ivory": "#fffff0",
    "aliceblue": "#f0f8ff", "royalblue": "#4169e1", "steelblue": "#4682b4", "dodgerblue": "#1e90ff",
    "midnightblue": "#191970", "darkblue": "#00008b", "crimson": "#dc143c", "firebrick": "#b22222",
    "darkred": "#8b0000", "tomato": "#ff6347", "darkgreen": "#006400", "seagreen": "#2e8b57",
    "forestgreen": "#228b22", "indigo": "#4b0082", "rebeccapurple": "#663399", "gold": "#ffd700",
    "beige": "#f5f5dc", "linen": "#faf0e6", "lavender": "#e6e6fa", "coral": "#ff7f50", "salmon": "#fa8072",
}


def parse_colour(text):
    """(r, g, b, a) with channels 0..255 and alpha 0..1, or None when the value is not understood."""
    text = text.strip().lower()
    if text == "transparent":
        return (0, 0, 0, 0.0)
    text = NAMED.get(text, text)
    if re.fullmatch(r"#[0-9a-f]{3,8}", text):
        digits = text[1:]
        if len(digits) in (3, 4):
            digits = "".join(char * 2 for char in digits)
        if len(digits) not in (6, 8):
            return None
        alpha = int(digits[6:8], 16) / 255 if len(digits) == 8 else 1.0
        return (int(digits[0:2], 16), int(digits[2:4], 16), int(digits[4:6], 16), alpha)
    match = re.fullmatch(r"(rgba?|hsla?)\(([^()]*)\)", text)
    if not match:
        return None
    numbers = [part for part in re.split(r"[\s,/]+", match.group(2).strip()) if part]
    if len(numbers) not in (3, 4):
        return None
    try:
        alpha = 1.0
        if len(numbers) == 4:
            alpha = float(numbers[3].rstrip("%")) / (100 if numbers[3].endswith("%") else 1)
        if match.group(1).startswith("rgb"):
            channels = [float(n.rstrip("%")) * (2.55 if n.endswith("%") else 1) for n in numbers[:3]]
        else:
            hue = float(re.sub(r"deg$", "", numbers[0])) % 360 / 360
            saturation, lightness = float(numbers[1].rstrip("%")) / 100, float(numbers[2].rstrip("%")) / 100
            import colorsys

            channels = [channel * 255 for channel in colorsys.hls_to_rgb(hue, lightness, saturation)]
    except ValueError:
        return None
    return (*[max(0.0, min(255.0, channel)) for channel in channels], max(0.0, min(1.0, alpha)))


def over(top, bottom):
    alpha = top[3]
    return tuple(top[i] * alpha + bottom[i] * (1 - alpha) for i in range(3)) + (1.0,)


def luminance(colour):
    def channel(value):
        value /= 255
        return value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4

    return 0.2126 * channel(colour[0]) + 0.7152 * channel(colour[1]) + 0.0722 * channel(colour[2])


def contrast(first, second):
    lighter, darker = sorted((luminance(first), luminance(second)), reverse=True)
    return (lighter + 0.05) / (darker + 0.05)


WHITE, BLACK = (255, 255, 255, 1.0), (0, 0, 0, 1.0)
UA_BACKGROUND = {"button": (239, 239, 239, 1.0), "input": WHITE, "select": WHITE, "textarea": WHITE, "dialog": WHITE}


def background_of(node, sheet):
    """The colour behind the node's text, or None when some layer of it cannot be worked out."""
    layers = []
    for current in [node, *node.ancestors()]:
        if current.tag == "#document":
            break
        value = sheet.declared(current).get("background-color")
        if value is None:
            colour = UA_BACKGROUND.get(current.tag)
            if current.tag == "input" and current.attrs.get("type") in ("submit", "button", "reset"):
                colour = UA_BACKGROUND["button"]
        elif value in ("inherit", "initial", "unset", "revert"):
            colour = None
        else:
            colour = parse_colour(value)
            if colour is None:
                return None
        if colour is not None and colour[3] > 0:
            layers.append(colour)
            if colour[3] >= 1:
                break
    result = WHITE
    for colour in reversed(layers):
        result = over(colour, result)
    return result


def colour_of(node, sheet):
    for current in [node, *node.ancestors()]:
        if current.tag == "#document":
            break
        value = sheet.declared(current).get("color")
        if value is None or value in ("inherit", "unset"):
            if value is None and current.tag in CONTROLS:
                return BLACK
            if value is None and current.tag == "a" and "href" in current.attrs:
                return (0, 0, 238, 1.0)
            continue
        if value in ("currentcolor", "currentColor"):
            continue
        return parse_colour(value)
    return BLACK


def to_px(value, parent=16.0):
    value = value.strip().lower()
    match = re.fullmatch(r"(-?[\d.]+)(px|rem|em|%|pt)?", value)
    if not match:
        return None
    number, unit = float(match.group(1)), match.group(2) or "px"
    return {"px": number, "rem": number * 16, "em": number * parent, "%": number * parent / 100, "pt": number * 4 / 3}[unit]


UA_FONT = {"h1": 2.0, "h2": 1.5, "h3": 1.17, "h5": 0.83, "h6": 0.67, "small": 0.83}


def font_px(node, sheet):
    chain = [current for current in [node, *node.ancestors()] if current.tag != "#document"]
    size = 16.0
    for current in reversed(chain):
        value = sheet.declared(current).get("font-size")
        if value is None:
            size = 13.33 if current.tag in CONTROLS else size * UA_FONT.get(current.tag, 1.0)
        elif value not in ("inherit", "unset"):
            inner = re.fullmatch(r"clamp\(([^,]+),.*\)", value)
            size = to_px(inner.group(1) if inner else value, size) or size
    return size


# ---------------------------------------------------------------------------------------------
# The checks. Each returns (passed, note).


def check_hooks(doc, by_id, html):
    problems = []
    for name, tag in REQUIRED_IDS.items():
        if name not in by_id:
            problems.append(f"#{name} is gone")
        elif tag and by_id[name].tag != tag:
            problems.append(f"#{name} is no longer a <{tag}>")
    if not problems:
        rows = [node for node in by_id["members"].walk() if "data-member" in node.attrs]
        if sorted(row.attrs["data-member"] for row in rows) != sorted(MEMBERS):
            problems.append("the data-member rows inside #members changed")
        if any(not any(n.attrs.get("data-action") == "remove" for n in row.walk()) for row in rows):
            problems.append("a member row has no data-action=remove")
        inside = {n.attrs.get("id") for n in by_id["invite-dialog"].walk()}
        if not {"invite-email", "invite-send", "invite-cancel"} <= inside:
            problems.append("the invite controls left #invite-dialog")
        form = {n.attrs.get("id") for n in by_id["settings-form"].walk()}
        if not {"display-name", "email", "timezone", "notify", "bio", "save"} <= form:
            problems.append("a settings field left #settings-form")
        if by_id["notify"].attrs.get("type") != "checkbox":
            problems.append("#notify is no longer a checkbox")
        if sorted(n.attrs.get("value") for n in by_id["timezone"].walk() if n.tag == "option") != ["cet", "est", "pst", "utc"]:
            problems.append("the time zone options changed")
        if "hidden" not in by_id["invite-dialog"].attrs and by_id["invite-dialog"].tag != "dialog":
            problems.append("#invite-dialog starts out open")
    if not any(n.tag == "script" and n.attrs.get("src", "").endswith("app.js") for n in doc.walk()):
        problems.append("app.js is no longer loaded")
    return not problems, "; ".join(problems)


def is_button(node):
    return node.tag == "button" or (node.tag == "input" and node.attrs.get("type") in ("button", "submit", "reset"))


def check_contrast(doc, sheet):
    failures, checked = [], 0
    for node in doc.walk():
        texted = node.text.strip() and node.tag not in ("script", "style", "title", "option", "head", "html", "#document")
        field = node.tag in ("input", "select", "textarea") and node.attrs.get("type") not in ("checkbox", "radio", "hidden", "range", "color", "file")
        if not (texted or field) or "disabled" in node.attrs:
            continue
        if any(a.tag in ("head", "script", "style") for a in node.ancestors()):
            continue
        colour, background = colour_of(node, sheet), background_of(node, sheet)
        if colour is None or background is None:
            continue
        checked += 1
        ratio = contrast(over(colour, background), background)
        if ratio < (3.0 if font_px(node, sheet) >= 24 else 4.5) - 0.005:
            label = node.attrs.get("id") or node.attrs.get("class") or node.text.strip()[:20]
            failures.append(f"<{node.tag}> {label!r} {ratio:.1f}:1")
    return not failures and checked > 0, f"{len(failures)} of {checked} texts too faint, e.g. " + ", ".join(failures[:3]) if failures else ""


def interactive(doc):
    return [n for n in doc.walk() if n.tag in CONTROLS and n.attrs.get("type") != "hidden" or n.tag == "a" and "href" in n.attrs]


def visible_outline(declared):
    outline = declared.get("outline")
    if outline is not None:
        return not re.search(r"\b(none|0|0px|hidden)\b", outline.lower()) or (re.search(r"\b[1-9]\d*(\.\d+)?(px|rem|em)\b", outline) and "none" not in outline)
    if declared.get("outline-style", "").lower() not in ("", "none", "hidden"):
        return True
    return None


def check_focus(doc, sheet):
    focus = frozenset({"focus", "focus-visible"})
    styled, suppressed = 0, []
    for node in interactive(doc):
        base, focused = sheet.declared(node), sheet.declared(node, focus)
        outline = visible_outline(focused)
        changed = any(
            focused.get(name) != base.get(name) and focused.get(name, "none").lower() not in ("none", "0")
            for name in ("box-shadow", "border-color", "border", "border-bottom", "border-bottom-color", "background-color", "text-decoration")
        )
        own_outline = outline and focused.get("outline", focused.get("outline-style")) != base.get("outline", base.get("outline-style"))
        if own_outline or changed:
            styled += 1
        elif outline is False or (outline is None and visible_outline(base) is False):
            suppressed.append(node.attrs.get("id") or node.tag)
    if suppressed:
        return False, "focus outline removed with nothing in its place: " + ", ".join(suppressed[:4])
    return styled > 0, "" if styled else "no :focus / :focus-visible style of its own"


def check_buttons(by_id, doc):
    wanted = [by_id[name] for name in ("save", "reset", "invite-open", "invite-send", "invite-cancel") if name in by_id]
    wanted += [n for n in doc.walk() if n.attrs.get("data-action") == "remove"]
    fake = [n.attrs.get("id") or n.attrs.get("data-action") for n in wanted if not is_button(n)]
    return not fake, "not real buttons: " + ", ".join(dict.fromkeys(fake)) if fake else ""


def check_labels(doc, by_id):
    labels = [n for n in doc.walk() if n.tag == "label"]
    unlabelled = []
    for node in doc.walk():
        if node.tag not in ("input", "select", "textarea") or node.attrs.get("type") in ("hidden", "submit", "button", "reset", "image"):
            continue
        named = node.attrs.get("aria-label", "").strip() or node.attrs.get("title", "").strip()
        named = named or any(by_id.get(ref) is not None and by_id[ref].all_text().strip() for ref in node.attrs.get("aria-labelledby", "").split())
        named = named or any(l.attrs.get("for") == node.attrs.get("id") and l.all_text().strip() for l in labels if node.attrs.get("id"))
        named = named or any(a.tag == "label" and a.all_text().strip() for a in node.ancestors())
        if not named:
            unlabelled.append(node.attrs.get("id") or node.tag)
    return not unlabelled, "no label: " + ", ".join(unlabelled) if unlabelled else ""


def check_no_inline(doc):
    inline = sum(1 for n in doc.walk() if "style" in n.attrs)
    blocks = sum(1 for n in doc.walk() if n.tag in ("style", "font", "center"))
    old = sum(1 for n in doc.walk() for a in ("bgcolor", "cellspacing", "cellpadding", "align", "border") if a in n.attrs)
    linked = any(n.tag == "link" and "stylesheet" in n.attrs.get("rel", "") and "styles.css" in n.attrs.get("href", "") for n in doc.walk())
    problems = [f"{inline} style attributes"] * bool(inline) + [f"{blocks} <style>/<font> elements"] * bool(blocks)
    problems += [f"{old} presentational attributes"] * bool(old) + ["styles.css is not linked"] * (not linked)
    return not problems, ", ".join(problems)


SPACING = re.compile(r"^(margin|padding)(-(top|right|bottom|left|inline|block)(-(start|end))?)?$|^(row-|column-)?gap$")


def length_key(token):
    token = token.lower()
    px = to_px(token.lstrip("-")) if not token.endswith(("em", "%")) or token.endswith("rem") else None
    if px is not None:
        return None if px == 0 else f"{round(px, 2):g}px"
    if token in ("auto", "inherit", "initial", "unset", "normal", "revert") or token.endswith("%"):
        return None
    return token.lstrip("-")


def check_spacing(sheet):
    values, raw_tokens = set(), []
    for rule in sheet.rules:
        for name, (value, _) in rule.declarations.items():
            if SPACING.match(name):
                raw_tokens += split_top(value, " ")
                values |= {length_key(token) for token in split_top(sheet.value(value), " ")} - {None}
    custom = {t for t in raw_tokens if t.startswith("var(")}
    plain = [t for t in raw_tokens if not t.startswith("var(") and length_key(t) is not None]
    tokenised = not plain and 0 < len(custom) <= MAX_SPACINGS + 2
    return len(values) <= MAX_SPACINGS or tokenised, f"{len(values)} different spacing values (at most {MAX_SPACINGS}): " + " ".join(sorted(values, key=lambda v: to_px(v) or 0))[:160]


def check_font_sizes(sheet):
    sizes = set()
    for rule in sheet.rules:
        for name, (value, _) in rule.declarations.items():
            for longhand, part in expand(name, sheet.value(value)):
                if longhand == "font-size" and part.lower() not in ("inherit", "initial", "unset", "100%", "1em", "revert"):
                    px = to_px(part) if not part.lower().endswith(("em", "%")) or part.lower().endswith("rem") else None
                    sizes.add(f"{round(px, 2):g}px" if px is not None else part.lower())
    return 0 < len(sizes) <= MAX_FONT_SIZES, f"{len(sizes)} different font sizes (at most {MAX_FONT_SIZES}): " + " ".join(sorted(sizes, key=lambda v: to_px(v) or 0))[:160]


def check_button_system(doc, sheet):
    buttons = [n for n in doc.walk() if is_button(n)]
    if len(buttons) < 5:
        return False, f"only {len(buttons)} real buttons to judge"
    radii, shapes = set(), set()
    for node in buttons:
        declared = sheet.declared(node)
        radii.add(declared.get("border-radius", "0").lower().replace(" ", ""))
        shapes.add(tuple(declared.get(f"padding-{side}", "ua").lower() for side in ("top", "right", "bottom", "left")) + (declared.get("font-size", "ua").lower(),))
    problems = []
    if len(radii) > 2:
        problems.append(f"{len(radii)} corner radii")
    if len(shapes) > 3:
        problems.append(f"{len(shapes)} button sizes")
    looks = ("background-color", "color", "border-color", "border", "opacity", "box-shadow", "filter", "text-decoration", "transform")
    for state, label in (("hover", "hover"), ("disabled", "disabled")):
        differs = sum(
            1
            for node in buttons
            if any(sheet.declared(node, frozenset({state})).get(p) != sheet.declared(node).get(p) for p in looks)
        )
        if differs < len(buttons) / 2:
            problems.append(f"no {label} style on most buttons")
    return not problems, ", ".join(problems)


def check_responsive(doc, sheet, css):
    viewport = any(n.tag == "meta" and n.attrs.get("name", "").lower() == "viewport" and "width=device-width" in n.attrs.get("content", "").replace(" ", "") for n in doc.walk())
    adaptive = any(rule.media and re.search(r"(min|max)-(width|inline-size)|width\s*[<>]", rule.media) for rule in sheet.rules)
    adaptive = adaptive or bool(re.search(r"clamp\(|minmax\(|auto-fi(t|ll)|flex-wrap\s*:\s*wrap", css))
    rigid = []
    for rule in sheet.rules:
        if rule.media is None:
            for name in ("width", "min-width"):
                px = to_px(sheet.value(rule.declarations.get(name, ("", False))[0])) if name in rule.declarations else None
                if px is not None and px >= 500 and not rule.declarations.get(name)[0].endswith("%"):
                    rigid.append(f"{rule.selector} {name}: {rule.declarations[name][0]}")
    problems = ["no viewport meta tag"] * (not viewport) + ["no media query or fluid layout"] * (not adaptive)
    problems += ["fixed widths: " + ", ".join(rigid[:2])] * bool(rigid)
    return not problems, ", ".join(problems)


def check_structure(doc, by_id):
    html = next((n for n in doc.walk() if n.tag == "html"), None)
    dialog = by_id.get("invite-dialog")
    named = dialog is not None and (dialog.attrs.get("aria-label", "").strip() or any(ref in by_id for ref in dialog.attrs.get("aria-labelledby", "").split()))
    points = {
        "lang on <html>": html is not None and bool(html.attrs.get("lang", "").strip()),
        "<th> table headers": any(n.tag == "th" for n in doc.walk()),
        "a named dialog": dialog is not None and (dialog.tag == "dialog" or dialog.attrs.get("role") in ("dialog", "alertdialog")) and bool(named),
        "a <main> landmark": any(n.tag == "main" or n.attrs.get("role") == "main" for n in doc.walk()),
        "a single <h1>": sum(1 for n in doc.walk() if n.tag == "h1") == 1,
    }
    missing = [name for name, ok in points.items() if not ok]
    return len(missing) <= 1, "missing: " + ", ".join(missing) if missing else ""


def main():
    try:
        html, css = open("index.html", encoding="utf-8").read(), open("styles.css", encoding="utf-8").read()
    except OSError as error:
        print(f"cannot read the page: {error}")
        print("score: 0/11")
        return 1
    tree = Tree()
    tree.feed(html)
    doc = tree.root
    by_id = {}
    for node in doc.walk():
        by_id.setdefault(node.attrs.get("id"), node)
    sheet = Sheet(css)

    checks = [
        ("hooks app.js relies on", lambda: check_hooks(doc, by_id, html)),
        ("text contrast 4.5:1", lambda: check_contrast(doc, sheet)),
        ("visible focus", lambda: check_focus(doc, sheet)),
        ("real buttons", lambda: check_buttons(by_id, doc)),
        ("labelled fields", lambda: check_labels(doc, by_id)),
        ("no inline styling", lambda: check_no_inline(doc)),
        ("spacing scale", lambda: check_spacing(sheet)),
        ("type scale", lambda: check_font_sizes(sheet)),
        ("one button system", lambda: check_button_system(doc, sheet)),
        ("responsive", lambda: check_responsive(doc, sheet, css)),
        ("document structure", lambda: check_structure(doc, by_id)),
    ]
    score, hooks_ok, failed = 0, False, []
    for index, (name, check) in enumerate(checks):
        try:
            passed, note = check()
        except Exception as error:  # a page the checker cannot read is a failed check, not a crash
            passed, note = False, f"could not be checked ({type(error).__name__}: {error})"
        score += bool(passed)
        if index == 0:
            hooks_ok = bool(passed)
        if not passed:
            failed.append(f"- {name}: {note}"[:230])
    print("\n".join(failed[-9:]))
    print(f"passed {score} of {len(checks)} checks (need {NEED}, and the hooks intact)")
    print(f"score: {score}/{len(checks)}")
    return 0 if hooks_ok and score >= NEED else 1


if __name__ == "__main__":
    sys.exit(main())
