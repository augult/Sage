# The `.conf` format

`conf.parse(text)` reads a whole file and returns its sections; `conf.load(path)` does the same
for a file on disk (UTF-8). Every problem in a file raises `conf.ConfError`, whose `line`
attribute is the 1-based number of the line at fault (see §9).

"Blank" below means a space or a tab. "Stripped" means without leading and trailing blanks.

## 1. Lines

Lines end in `\n` or `\r\n`. A line that is empty or all blanks is ignored. A line whose first
non-blank character is `#` or `;` is a comment and is ignored.

## 2. Sections

`[name]` starts a section; the entries after it belong to it. Blanks around the brackets and
inside them are ignored, so `  [ db.primary ]  ` starts section `db.primary`. Nothing else may be
on the line, not even a comment.

A name is made of ASCII letters, digits, `_`, `-` and `.` and is not empty. Names are
case-sensitive. A header may occur more than once: its entries are added to the same section.

Entries before the first header belong to the top section, whose name is the empty string.

## 3. Entries

`key = value`. The key is the stripped text before the **first** `=`; it follows the same rules as
a section name. The value is the stripped text after that `=`, and may be empty. Further `=`
signs are part of the value.

A line that is not blank, not a comment, not a header and has no `=` is an error.

Giving the same key twice in one section is an error.

## 4. Plain values and inline comments

A value that does not start with `"` is plain. A `#` or `;` inside it starts a comment if the
character before it is a blank; the comment and the blanks before it are dropped.

    path = /srv/app   # where it lives       ->  /srv/app
    colour = #ff8800                         ->  #ff8800      (first character of the value)
    tag = a#b ;c                             ->  a#b
    note = # not a comment                   ->  # not a comment

## 5. Continuation lines

If a plain value's line ends in a backslash (blanks after it do not count), the entry continues
on the next line. The pieces are joined like this: the backslash is dropped, both pieces are
stripped, and they are joined with a single space. The next line is taken as it is, whatever it
looks like: it is never a comment, a header or an entry of its own. It may end in a backslash
too, and so on.

Inline comments (§4) are looked for **after** joining, in the joined text.

    hosts = alpha \
            beta \
            gamma   # three of them          ->  alpha beta gamma

A backslash at the end of the last line of the file is an error. An empty piece adds nothing, not
even a space.

## 6. Quoted values

A value that starts with `"` is quoted and ends at the next `"` that is not escaped. Everything
between the quotes is kept as it is, blanks, `#` and `;` included, except for these escapes:

    \\   backslash        \"   double quote      \n   newline      \t   tab
    \uXXXX   the character with that code point, exactly four hexadecimal digits

Any other backslash sequence is an error, and so is a missing closing quote. After the closing
quote the line may only hold blanks, optionally followed by a comment starting with `#` or `;`.
Quoted values do not continue on the next line.

    greeting = "  hello # world  "   ; kept as typed   ->  "  hello # world  " (without the quotes)

## 7. Lists

A key written as `name[]` adds its value to the list `name` of the section, in file order. The
value follows the same rules as any other. A section cannot have both `name` and `name[]`
entries; whichever comes second is the error.

    [server]
    listen[] = 127.0.0.1:8080
    listen[] = "[::1]:8080"

## 8. References

After the whole file is read, references in values are replaced:

- `${key}` is the value of `key` in the same section or, if the section has no such key, in the
  top section.
- `${section:key}` is the value of `key` in that section. `${:key}` names the top section.
- `$$` is a literal `$`. A `$` followed by anything else than `{` or `$` stands for itself.

References may point forward or backward, and the value they point at has its own references
replaced first. The replaced text is not scanned again. Quoted and plain values, and the items of
a list, are all treated the same; escapes (§6) are processed before references.

These are errors: a reference to a key or section that does not exist, a reference to a list, a
`${` without a closing `}`, and a value that refers to itself, directly or through others.

    base = /srv
    [app]
    data = ${base}/data          ->  /srv/data
    log = ${app:data}/log        ->  /srv/data/log
    price = 5$$ or $5            ->  5$ or $5

## 9. Line numbers of errors

- A problem with a header or an entry: the line the header or the entry **starts** on (for a
  continued entry that is its first line, whichever piece is at fault).
- A duplicate key, or a mix of `name` and `name[]`: the line of the later entry.
- A bad reference: the line where the entry holding the reference starts. For a cycle, the line
  of any entry that takes part in it.

## 10. The result

`parse` returns a `dict` of sections in order of first appearance. Each section is a `dict` of
keys in order of first appearance; a value is a `str`, or a `list` of `str` for a list. List keys
appear without the `[]`. The top section is present only if it has entries. A section whose
header occurs but which has no entries is present and empty.

A byte-order mark (`\ufeff`) at the very start of the text is ignored.
