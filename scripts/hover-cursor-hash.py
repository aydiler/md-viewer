#!/usr/bin/env python3
"""Print a hash of the current X cursor image, for scripts/hover-stability.sh.

The cursor is the second observable in issue #139 — the report describes the
resize cursor alternating as well as the scrollbar changing width, and both
turned out to reproduce at the same single pixel. Sampling it needs XFixes,
which python-xlib exposes; without that module the caller degrades to the
pixel half alone rather than failing.
"""
import hashlib
import sys

try:
    from Xlib import display
except ImportError:  # pragma: no cover - depends on the host
    print("NOXLIB")
    sys.exit(0)

d = display.Display(sys.argv[1] if len(sys.argv) > 1 else None)
if not d.has_extension("XFIXES"):
    print("NOXFIXES")
    sys.exit(0)
d.xfixes_query_version()
image = d.xfixes_get_cursor_image(d.screen().root)
digest = hashlib.sha1(bytes(str(image.cursor_image), "utf8")).hexdigest()[:8]
print(digest, getattr(image, "width", "?"), getattr(image, "height", "?"))
