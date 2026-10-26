"""Small client for the Immich server API (photos, tags, albums, faces)."""
from __future__ import annotations

import os
from datetime import datetime, timezone

import requests

IMMICH_URL = os.environ.get("IMMICH_URL", "http://immich-server:2283").rstrip("/") + "/api"


class Immich:
    def __init__(self, api_key: str | None = None, token: str | None = None, url: str = IMMICH_URL):
        self.url = url
        self.s = requests.Session()
        if api_key:
            self.s.headers["x-api-key"] = api_key
        elif token:
            self.s.headers["Authorization"] = f"Bearer {token}"

    def _req(self, method: str, path: str, **kw):
        r = self.s.request(method, f"{self.url}{path}", timeout=kw.pop("timeout", 120), **kw)
        if r.status_code >= 400:
            raise RuntimeError(f"Immich {method} {path} -> {r.status_code}: {r.text[:300]}")
        return r

    # ---- photos -----------------------------------------------------------
    def assets(self, page: int = 1, size: int = 100, **filters) -> tuple[list[dict], int | None]:
        body = {"page": page, "size": size, "withExif": True, **filters}
        d = self._req("POST", "/search/metadata", json=body).json()["assets"]
        nxt = d.get("nextPage")
        return d["items"], int(nxt) if nxt else None

    def all_assets(self, **filters) -> list[dict]:
        out, page = [], 1
        while page:
            items, page = self.assets(page=page, size=250, **filters)
            out += items
        return out

    def asset(self, asset_id: str) -> dict:
        return self._req("GET", f"/assets/{asset_id}").json()

    def preview(self, asset_id: str, size: str = "preview") -> bytes:
        return self._req("GET", f"/assets/{asset_id}/thumbnail", params={"size": size}).content

    def original(self, asset_id: str) -> bytes:
        return self._req("GET", f"/assets/{asset_id}/original", timeout=600).content

    def upload(self, data: bytes, filename: str, created: str | None = None) -> str:
        now = datetime.now(timezone.utc).isoformat()
        r = self._req("POST", "/assets", files={"assetData": (filename, data)},
                      data={"deviceAssetId": f"studio-{filename}-{now}", "deviceId": "immich-studio",
                            "fileCreatedAt": created or now, "fileModifiedAt": now}, timeout=600)
        return r.json()["id"]

    def delete(self, asset_ids: list[str], forever: bool = False):
        self._req("DELETE", "/assets", json={"ids": asset_ids, "force": forever})

    def set_description(self, asset_id: str, text: str):
        self._req("PUT", f"/assets/{asset_id}", json={"description": text})

    # ---- tags -------------------------------------------------------------
    def upsert_tags(self, names: list[str]) -> dict[str, str]:
        """Create tags like "Environment/Beach" (nested) and return name -> id."""
        if not names:
            return {}
        r = self._req("PUT", "/tags", json={"tags": names}).json()
        return {t["value"]: t["id"] for t in r}

    def tag_assets(self, tag_ids: list[str], asset_ids: list[str]):
        if tag_ids and asset_ids:
            self._req("PUT", "/tags/assets", json={"tagIds": tag_ids, "assetIds": asset_ids})

    def remove_tag_prefix(self, asset_id: str, prefix: str):
        for t in self.asset(asset_id).get("tags", []) or []:
            if t["value"].startswith(prefix):
                self._req("DELETE", f"/tags/{t['id']}/assets", json={"ids": [asset_id]})

    # ---- albums -----------------------------------------------------------
    def albums(self) -> list[dict]:
        return self._req("GET", "/albums").json()

    def album_asset_ids(self, album_id: str) -> list[str]:
        return [a["id"] for a in self.all_assets(albumIds=[album_id])]

    def ensure_album(self, name: str, asset_ids: list[str]) -> str:
        existing = {a["albumName"]: a["id"] for a in self.albums()}
        if name in existing:
            if asset_ids:
                self._req("PUT", f"/albums/{existing[name]}/assets", json={"ids": asset_ids})
            return existing[name]
        return self._req("POST", "/albums", json={"albumName": name, "assetIds": asset_ids}).json()["id"]

    # ---- people -----------------------------------------------------------
    def faces(self, asset_id: str) -> list[dict]:
        return self._req("GET", "/faces", params={"id": asset_id}).json()

    def people(self) -> list[dict]:
        return self._req("GET", "/people", params={"withHidden": "false"}).json()["people"]
