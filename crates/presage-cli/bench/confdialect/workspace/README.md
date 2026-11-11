# conf

Reader for our `.conf` settings files. `SPEC.md` defines the format; `examples/app.conf` shows
most of it in one file.

    python3 -m conf examples/app.conf          # the file as JSON
    python3 -m unittest discover -s tests

```python
import conf

settings = conf.load("examples/app.conf")
settings["server"]["listen"]      # ['127.0.0.1:8080', '[::1]:8080']
```
