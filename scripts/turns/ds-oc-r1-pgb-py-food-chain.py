THOUGHT = r'''Generation matches the README lyric block line for line. Done.'''

CONTENT = r'''`food_chain.py` implements `recite(start_verse, end_verse)` returning a list of verses, each a list of lines (the Python-track shape).

An ordered animal table drives generation: each verse names the animal, adds its remark, then walks the chain down to the fly with `She swallowed the ... to catch the ...`, giving the spider its "that wriggled and jiggled and tickled inside her" clause. The horse verse stops after "She's dead, of course!". Out-of-range or reversed requests raise `ValueError`.

Generated all eight verses and matched them line by line against the lyrics in `README.md`.'''

CALLS = []
