THOUGHT = "I’ll replace the parser with a valid source patch, covering validation, values, lists, and references."
CONTENT = ""
PATCH = """*** Begin Patch
*** Add File: conf/parser.py
\"\"\"Parser for the .conf format.\"\"\"
import re
from .errors import ConfError

NAME = re.compile(r\"[A-Za-z0-9_.-]+\\Z\")

def fail(message, line):
    raise ConfError(message, line)

def parse(text):
    if text.startswith('\\ufeff'):
        text = text[1:]
    lines = text.splitlines()
    sections, origins = {}, {}
    current = None
    i = 0
    while i < len(lines):
        n, line = i + 1, lines[i]
        s = line.strip(' \\t')
        if not s or s[0] in '#;':
            i += 1
            continue
        if s.startswith('['):
            m = re.fullmatch(r\"\\[\\s*([^]\\r\\n]*?)\\s*\\]\", s)
            if not m or not NAME.fullmatch(m.group(1)):
                fail('invalid section header', n)
            current = m.group(1)
            sections.setdefault(current, {})
            i += 1
            continue
        if '=' not in line:
            fail('expected `key = value`', n)
        key, raw = line.split('=', 1)
        key = key.strip(' \\t')
        many = key.endswith('[]')
        name = key[:-2] if many else key
        if not NAME.fullmatch(name):
            fail('invalid key', n)
        sec = current if current is not None else ''
        entries = sections.setdefault(sec, {})
        raw = raw.strip(' \\t')
        if not raw.startswith('\\\"'):
            while raw.rstrip(' \\t').endswith('\\\\'):
                piece = raw.rstrip(' \\t')[:-1].strip(' \\t')
                i += 1
                if i >= len(lines): fail('continuation at end of file', n)
                raw = piece + ' ' + lines[i].strip(' \\t')
        if raw.startswith('\\\"'):
            out, j = [], 1
            while j < len(raw):
                c = raw[j]
                if c == '\\\"':
                    tail = raw[j+1:].strip(' \\t')
                    if tail and tail[0] not in '#;': fail('unexpected text after quote', n)
                    break
                if c != '\\\\\\\\': out.append(c); j += 1; continue
                j += 1
                if j >= len(raw): fail('invalid escape', n)
                e = raw[j]
                if e in ('\\\\\\\\', '\\\"'): out.append(e); j += 1
                elif e == 'n': out.append('\\n'); j += 1
                elif e == 't': out.append('\\t'); j += 1
                elif e == 'u':
                    h = raw[j+1:j+5]
                    if not re.fullmatch(r\"[0-9a-fA-F]{4}\", h): fail('invalid unicode escape', n)
                    out.append(chr(int(h, 16))); j += 5
                else: fail('invalid escape', n)
            else: fail('missing closing quote', n)
            value = ''.join(out)
        else:
            value = raw
            for k, c in enumerate(value):
                if c in '#;' and k and value[k-1] in ' \\t':
                    value = value[:k].rstrip(' \\t'); break
        if name in entries:
            if many != isinstance(entries[name], list): fail('cannot mix scalar and list', n)
            if not many: fail('duplicate key', n)
            entries[name].append(value)
        else:
            entries[name] = [value] if many else value
            origins[(sec, name)] = n
        i += 1
    if '' in sections and not sections['']: del sections['']
    active, cache = [], {}
    def resolve(sec, key, owner):
        if sec not in sections or key not in sections[sec]: fail('missing reference', owner)
        val = sections[sec][key]
        if isinstance(val, list): fail('reference to list', owner)
        ident = (sec, key)
        if ident in active: fail('cyclic reference', origins[ident])
        if ident in cache: return cache[ident]
        active.append(ident)
        result = expand(val, sec, origins[ident])
        active.pop()
        cache[ident] = result
        return result
    def expand(value, sec, owner):
        out, j = [], 0
        while j < len(value):
            if value[j:j+2] == '$$': out.append('$'); j += 2
            elif value[j:j+2] == '${':
                end = value.find('}', j+2)
                if end < 0: fail('reference missing closing brace', owner)
                ref = value[j+2:end]
                if ':' in ref:
                    target, key = ref.split(':', 1)
                    if target and not NAME.fullmatch(target) or not NAME.fullmatch(key): fail('invalid reference', owner)
                else:
                    key = ref
                    if not NAME.fullmatch(key): fail('invalid reference', owner)
                    target = sec if key in sections.get(sec, {}) else ''
                out.append(resolve(target, key, owner)); j = end + 1
            else: out.append(value[j]); j += 1
        return ''.join(out)
    for sec, entries in sections.items():
        for key, value in entries.items():
            if isinstance(value, list): entries[key] = [expand(v, sec, origins[(sec,key)]) for v in value]
            else: entries[key] = resolve(sec, key, origins[(sec,key)])
    return sections

def load(path):
    with open(path, encoding='utf-8') as source:
        return parse(source.read())
*** End Patch"""
CALLS = [{"name": "apply_patch", "arguments": {"patchText": PATCH}}, {"name": "bash", "arguments": {"command": "python3 -m unittest discover -s tests", "timeout": 30}}]
