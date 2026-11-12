# textstats

Small helpers for counting words.

- `words(text)`: the lower-cased words of a text, in order.
- `word_count(text)`: how many words it has.
- `top_words(text, n=3)`: **not implemented yet.** The `n` most frequent words as a list of
  `(word, count)` pairs, most frequent first. Words with the same count are ordered
  alphabetically. If the text has fewer than `n` distinct words, all of them are returned.
  `n` below 1 raises `ValueError`.

Run the tests with `python3 -m unittest`.
