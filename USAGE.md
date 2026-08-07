# USAGE — Getting High-Quality Tidal URLs

> Draft — documents how to obtain playable, high-quality audio URLs from a running
> `hifi-api` instance (default `http://localhost:8000`), including lossless FLAC,
> 24-bit hi-res, and Dolby Atmos streams.

---

## 1. Quality tiers

Tidal serves audio in several tiers. Which one you get depends on **both** the
track's availability and your account/client entitlement:

| Tier | Format | Typical specs | Notes |
|---|---|---|---|
| `HIGH` | AAC (`mp4a.40.2`) | 320 kbps | The fallback. The v1 API often caps even entitled accounts here. |
| `LOSSLESS` / `FLAC` | FLAC-in-fMP4 | 16-bit / 44.1 kHz | CD quality. |
| `FLAC_HIRES` | FLAC-in-fMP4 | 24-bit / 96–192 kHz (e.g. 176.4 kHz) | Only on tracks tagged `HIRES_LOSSLESS`; client must be entitled. |
| `EAC3_JOC` | E-AC-3 (Dolby Atmos) | 48 kHz, 5.1/7.1 bed + JOC objects | Only on tracks tagged `DOLBY_ATMOS`; client must be entitled. |

Tidal silently drops formats you aren't entitled to when multiple are requested,
and returns `403 CLIENT_NOT_ENTITLED` if you request only unentitled ones.

---

## 2. Find a track ID

```bash
curl -s "http://localhost:8000/search/?s=Billie%20Jean" | \
  python3 -c "import json,sys; [print(i['id'], i['title'], '-', i['artist']['name']) for i in json.load(sys.stdin)['data']['items'][:5]]"
```

```
1781887 Billie Jean - Michael Jackson            (Thriller 1982 — HIRES)
522737219 Billie Jean - Michael Jackson          (2022 Atmos release — DOLBY_ATMOS)
```

Search results also expose `mediaMetadata.tags`, which tell you the highest tier
the track supports (`LOSSLESS`, `HIRES_LOSSLESS`, `DOLBY_ATMOS`).

---

## 3. Method A — `/dash/{id}` (recommended, highest quality)

Returns a **302 redirect** to a fresh Tidal DASH manifest, requesting
`FLAC_HIRES,FLAC,EAC3_JOC,AACLC` in that priority order:

```bash
curl -s -D - -o /dev/null "http://localhost:8000/dash/1781887"
```

```http
HTTP/1.1 307 Temporary Redirect
location: https://im-fa.manifest.tidal.com/1/manifests/EgcxNzgxODg3GAI...mpd?token=...
```

The MPD contains one `Representation` per entitled format. For Billie Jean:

```
Representation id="FLAC_HIRES,176400,24"  codecs="flac"  bandwidth="5962169"  24-bit / 176.4 kHz
Representation id="FLAC,44100,16"         codecs="flac"  bandwidth="893519"   16-bit / 44.1 kHz
```

Paste the redirect target (or the `/dash/{id}` URL itself — players follow
redirects) into any DASH-capable player:

```bash
mpv "http://localhost:8000/dash/1781887"        # picks FLAC_HIRES automatically
ffplay "http://localhost:8000/dash/1781887"
```

For the Atmos edition:

```bash
mpv "http://localhost:8000/dash/522737219"      # E-AC-3 5.1 bed (JOC not rendered by mpv)
```

---

## 4. Method B — `/trackManifests/{id}` (raw v2 API)

The underlying v2 endpoint, exposed directly. Useful when you need the full JSON
(URI, hash, DRM data, normalization) rather than a redirect:

```bash
curl -s "http://localhost:8000/trackManifests/1781887?formats=FLAC_HIRES,FLAC,AACLC" | \
  python3 -m json.tool
```

Query parameters (all optional):

| Param | Default | Notes |
|---|---|---|
| `formats` | `HEAACV1,AACLC,FLAC,FLAC_HIRES,EAC3_JOC` | Comma-separated. Unentitled ones are dropped. |
| `adaptive` | `true` | Multi-format response. |
| `manifestType` | `MPEG_DASH` | `MPEG_DASH` or `HLS`. |
| `uriScheme` | `HTTPS` | `HTTPS` = manifest link, `DATA` = inline base64. |
| `usage` | `PLAYBACK` | `PLAYBACK` or `DOWNLOAD`. |

Response shape:

```json
{
  "version": "2.10",
  "data": {
    "data": {
      "id": "1781887",
      "type": "trackManifests",
      "attributes": {
        "trackPresentation": "FULL",
        "uri": "https://im-fa.manifest.tidal.com/1/manifests/...mpd?token=...",
        "hash": "...",
        "formats": ["FLAC_HIRES", "FLAC", "AACLC"]
      }
    }
  }
}
```

---

## 5. Method C — `/track/{id}` (direct file, AAC only)

The legacy v1 endpoint returns a **single direct MP4 file URL** (no DASH, no DRM):

```bash
curl -s "http://localhost:8000/track/?id=1781887&quality=HI_RES_LOSSLESS" | \
  python3 -c "
import json, sys, base64
d = json.load(sys.stdin)['data']
m = json.loads(base64.b64decode(d['manifest']))
print(d['audioQuality'])   # HIGH — see warning below
print(m['urls'][0])        # direct .mp4 URL, token expires ~1h
"
```

> ⚠️ **Warning:** Tidal currently caps v1 `playbackinfo` at `HIGH` (320 kbps AAC)
> for most accounts/clients, regardless of the `quality` parameter. Use
> Method A/B for lossless. The direct file is useful for players with no DASH
> support (simple HTTP audio).

---

## 6. Known limitations

- **Entitlements are per client + track.** `403 CLIENT_NOT_ENTITLED` means the
  configured Tidal client credentials can't access that tier for that track
  (common for `FLAC_HIRES` on some tracks and for hi-res in general). Not fixable
  in code.
- **Tokens expire (~1 h).** Manifest and segment URLs embed expiring tokens.
  Always re-request `/dash/{id}` or `/trackManifests/{id}` for a fresh URL —
  don't cache the redirect target.
- **Atmos rendering.** The `EAC3_JOC` stream's object metadata (JOC) is only
  rendered by a Dolby Atmos renderer (e.g. Android TV via ExoPlayer). mpv/VLC
  decode the E-AC-3 5.1/7.1 bed layer.
- **High sample rates** (176.4/192 kHz) decode fine in ffmpeg-based players;
  final output may be resampled by your sound server/DAC.
- **DRM:** tracks whose manifest includes `drmData` (Widevine) require the
  `/widevine` proxy + a DRM-capable player; the FLAC/hi-res/EAC3 streams in
  Methods A–C are currently served unencrypted.

---

## 7. Quick reference

```bash
# Play highest quality of a track
mpv "http://localhost:8000/dash/1781887"

# Inspect available tiers of a track
curl -s "http://localhost:8000/search/?s=Billie%20Jean" | grep -o '"tags":\[[^]]*\]'

# Get the raw v2 manifest JSON
curl -s "http://localhost:8000/trackManifests/1781887"
```
