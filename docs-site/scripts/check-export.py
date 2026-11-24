"""Check exported routes, internal HTML links and fragment targets."""

from html.parser import HTMLParser
import json
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit

SITE = Path(__file__).resolve().parents[1]
OUT = SITE / "out"
HOST = "reddit.micr.dev"


class Document(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.links = set()
        self.anchors = set()
        self.feed(path.read_text(encoding="utf-8"))
        self.close()

    def handle_starttag(self, tag, attributes):
        for key, value in attributes:
            if value and key in ("href", "src"):
                self.links.add(value)
            elif value and (key == "id" or (tag == "a" and key == "name")):
                self.anchors.add(value)


def main():
    expected = {"index.html", "api/search", "llms.txt", "CNAME"}
    content = SITE / "content"
    for path in (content / "docs").rglob("*.mdx"):
        route = path.relative_to(content).with_suffix("")
        if route.name == "index":
            route = route.parent
        expected.add(str(route / "index.html"))
    missing = sorted(
        path for path in expected
        if not (OUT / path).is_file() or not (OUT / path).stat().st_size
    )
    if missing:
        raise SystemExit("missing or empty exports:\n" + "\n".join(missing))
    if (OUT / "CNAME").read_text().strip() != HOST:
        raise SystemExit("unexpected CNAME")
    if not json.loads((OUT / "api/search").read_text(encoding="utf-8")):
        raise SystemExit("empty search index")

    docs = {path: Document(path) for path in OUT.rglob("*.html")}
    broken = []
    links = 0
    for path, doc in docs.items():
        relative = path.relative_to(OUT).as_posix()
        url = f"https://{HOST}/{relative.removesuffix('index.html')}"
        for href in sorted(doc.links):
            target = urlsplit(urljoin(url, href))
            if target.scheme not in ("http", "https") or target.netloc != HOST:
                continue
            links += 1
            dest = OUT / unquote(target.path).lstrip("/")
            if dest.is_dir():
                dest /= "index.html"
            fragment = unquote(target.fragment)
            if not dest.is_file() or (fragment and dest in docs and fragment not in docs[dest].anchors):
                broken.append(f"{relative}: {href}")

    if broken:
        raise SystemExit("broken internal links:\n" + "\n".join(broken))
    print(f"verified {len(expected)} required exports and {links} internal links across {len(docs)} HTML pages")


if __name__ == "__main__":
    main()
