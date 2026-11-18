# Store listings

Everything is regenerated from this folder; nothing here is uploaded automatically.

- `msstore/listing.csv`, `msstore/images/`, `guitar-fx-msstore-listing.zip`: the Microsoft Store listing
  (product `9P816T97D1CC`). In Partner Center choose **Store listings → Import / export → Import**, and upload the zip.
  Then review the text, set the category and age rating, and add the privacy policy URL.
- `snap/`: Snap Store artwork and `listing.md` with every dashboard field.
- `make_assets.py` draws the logos and banners and resizes screenshots; `make_listing.py` fills the CSV from the
  empty export. Screenshots were captured from the real UI at 1920x1080 (software rendering in a virtual display).

Not covered by the listing files: category, age rating, pricing, markets and the privacy policy URL
(https://github.com/alfu32/guitar-fx/blob/master/PRIVACY.md) are entered in Partner Center directly.
