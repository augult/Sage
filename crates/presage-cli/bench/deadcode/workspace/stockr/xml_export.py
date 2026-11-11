"""The XML report the old warehouse terminal read."""

from xml.sax.saxutils import escape


def to_xml(summary):
    lines = ["<report>"]
    for key in ("items", "units", "value"):
        lines.append(f"  <{key}>{escape(str(summary[key]))}</{key}>")
    lines.append("</report>")
    return "\n".join(lines) + "\n"
