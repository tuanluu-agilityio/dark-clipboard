"""Generates docs/architecture/schemas/*.schema.json (JSON Schema 2020-12).

Single source for the Phase 0 entity schemas. Run: python scripts/gen_schemas.py
Validate examples afterwards: node scripts/validate-schemas.mjs
"""
import json
import pathlib

B = "https://darkclipboard.dev/schemas"
OUT = pathlib.Path(__file__).resolve().parent.parent / "docs" / "architecture" / "schemas"
D = "2020-12"


def doc(name, title, body, description=None):
    d = {"$schema": f"https://json-schema.org/draft/{D}/schema", "$id": f"{B}/{name}", "title": title}
    if description:
        d["description"] = description
    d.update(body)
    return name, d


C = "common.schema.json#/$defs/"
ref = lambda n: {"$ref": C + n}  # noqa: E731
obj = lambda required, props, **kw: {"type": "object", "additionalProperties": False, "required": required, "properties": props, **kw}  # noqa: E731
V1 = {"const": 1}

schemas = [
    doc("common.schema.json", "Common definitions", {"$defs": {
        "uuid": {"type": "string", "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$", "description": "UUID v7, lowercase"},
        "timestamp": {"type": "string", "format": "date-time", "pattern": "Z$"},
        "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
        "clipType": {"enum": ["text", "rich_text", "url", "color", "code", "image", "files"]},
        "platform": {"enum": ["windows", "macos", "linux-x11", "linux-wayland"]},
        "sensitiveCategory": {"enum": ["api_key", "token", "private_key", "password", "payment_card", "email", "phone"]},
        "scope": {"enum": ["clips:read-metadata", "clips:read-content", "collections:read", "collections:write", "clips:write-organization", "clipboard:write", "export"]},
        "ref": obj(["id", "name"], {"id": {"$ref": "#/$defs/uuid"}, "name": {"type": "string", "minLength": 1, "maxLength": 80}}),
    }}),
    doc("clipboard-source.schema.json", "ClipboardSource", obj(["app_id", "platform", "confidence"], {
        "app_id": {"type": "string", "minLength": 1, "maxLength": 256},
        "app_name": {"type": "string", "maxLength": 256},
        "platform": ref("platform"),
        "confidence": {"enum": ["owner", "frontmost", "declared"]},
    }), "Where a clip came from. Never contains window titles."),
    doc("clip.schema.json", "Clip", {
        **obj(
            ["schema_version", "id", "kind", "preview", "representations", "content_hash", "size_bytes", "created_at",
             "last_copied_at", "copy_count", "use_count", "pinned", "sensitivity", "metadata"],
            {
                "schema_version": V1,
                "id": ref("uuid"),
                "kind": ref("clipType"),
                "preview": {"type": "string", "maxLength": 512},
                "representations": {"type": "array", "minItems": 1, "items": {"$ref": "#/$defs/representation"}},
                "content_hash": ref("sha256"),
                "size_bytes": {"type": "integer", "minimum": 0},
                "created_at": ref("timestamp"),
                "last_copied_at": ref("timestamp"),
                "last_used_at": ref("timestamp"),
                "copy_count": {"type": "integer", "minimum": 1},
                "use_count": {"type": "integer", "minimum": 0},
                "pinned": {"type": "boolean"},
                "pin_order": {"type": "integer", "minimum": 0},
                "source": {"$ref": "clipboard-source.schema.json"},
                "sensitivity": obj(["level", "categories"], {
                    "level": {"enum": ["none", "masked"]},
                    "categories": {"type": "array", "uniqueItems": True, "items": ref("sensitiveCategory")},
                }),
                "note": {"type": "string", "maxLength": 4000},
                "tags": {"type": "array", "items": ref("ref")},
                "collections": {"type": "array", "items": ref("ref")},
                "expires_at": ref("timestamp"),
                "metadata": {"$ref": "#/$defs/metadata"},
                "content": {
                    **obj(["text"], {"text": {"type": "string"}, "truncated": {"type": "boolean"}}),
                    "description": "Present only when requested by a caller holding clips:read-content. Text only; images and files are referenced, never inlined. Never present for masked clips.",
                },
            },
        ),
        "if": {"properties": {"sensitivity": {"type": "object", "properties": {"level": {"const": "masked"}}}}},
        "then": {"not": {"required": ["content"]}, "required": ["expires_at"]},
        "$defs": {
            "representation": {
                **obj(["mime", "size_bytes", "storage"], {
                    "mime": {"enum": ["text/plain;charset=utf-8", "text/html", "text/rtf", "image/png", "text/uri-list"]},
                    "platform_format": {"type": "string", "maxLength": 128},
                    "size_bytes": {"type": "integer", "minimum": 0},
                    "storage": {"enum": ["inline", "blob"]},
                    "blob_sha256": ref("sha256"),
                    "width": {"type": "integer", "minimum": 1},
                    "height": {"type": "integer", "minimum": 1},
                }),
                "if": {"properties": {"storage": {"const": "blob"}}},
                "then": {"required": ["blob_sha256"]},
                "else": {"not": {"required": ["blob_sha256"]}},
            },
            "metadata": {"type": "object", "additionalProperties": False, "properties": {
                "char_count": {"type": "integer", "minimum": 0},
                "line_count": {"type": "integer", "minimum": 0},
                "file_count": {"type": "integer", "minimum": 0},
                "language_hint": {"type": "string", "maxLength": 32},
                "image_thumb_sha256": ref("sha256"),
                "truncated": {"type": "boolean"},
            }},
        },
    }),
    doc("collection.schema.json", "Collection", {
        **obj(["schema_version", "id", "name", "kind", "created_at", "updated_at", "clip_count", "sort"], {
            "schema_version": V1,
            "id": ref("uuid"),
            "name": {"type": "string", "minLength": 1, "maxLength": 80},
            "kind": {"enum": ["user", "system"]},
            "system_key": {"enum": ["favorites"]},
            "color": {"type": "string", "pattern": "^#[0-9a-f]{6}$"},
            "created_at": ref("timestamp"),
            "updated_at": ref("timestamp"),
            "clip_count": {"type": "integer", "minimum": 0},
            "sort": {"enum": ["manual", "recent"]},
        }),
        "if": {"properties": {"kind": {"const": "system"}}},
        "then": {"required": ["system_key"]},
        "else": {"not": {"required": ["system_key"]}},
    }, "User or system collection. Favorites is the system collection with system_key = favorites."),
    doc("tag.schema.json", "Tag", obj(["schema_version", "id", "name", "created_at"], {
        "schema_version": V1,
        "id": ref("uuid"),
        "name": {"type": "string", "minLength": 1, "maxLength": 40, "pattern": "^[^A-Z\\s]+$", "description": "Lowercase; letters, digits, _ and - (enforced fully in darkclip-core)"},
        "created_at": ref("timestamp"),
        "clip_count": {"type": "integer", "minimum": 0},
    })),
    doc("retention-policy.schema.json", "RetentionPolicy", obj(
        ["schema_version", "max_items", "max_age_days", "max_storage_mb", "keep_pinned", "keep_favorites", "sensitive_ttl_seconds", "image_max_mb", "text_max_kb"], {
            "schema_version": V1,
            "max_items": {"type": "integer", "minimum": 0, "maximum": 1000000, "description": "0 = unlimited"},
            "max_age_days": {"type": "integer", "minimum": 0, "maximum": 3650, "description": "0 = unlimited"},
            "max_storage_mb": {"type": "integer", "minimum": 10, "maximum": 102400},
            "keep_pinned": {"const": True},
            "keep_favorites": {"type": "boolean"},
            "sensitive_ttl_seconds": {"type": "integer", "minimum": 10, "maximum": 3600},
            "image_max_mb": {"type": "integer", "minimum": 1, "maximum": 200},
            "text_max_kb": {"type": "integer", "minimum": 1, "maximum": 16384},
        })),
    doc("privacy-policy.schema.json", "PrivacyPolicy", {
        **obj(["schema_version", "capture_enabled", "mode", "excluded_apps", "capture_kinds", "sensitive_handling",
               "sensitive_categories_enabled", "respect_os_markers", "capture_primary_selection"], {
            "schema_version": V1,
            "capture_enabled": {"type": "boolean"},
            "mode": {"enum": ["normal", "paused", "private"]},
            "paused_until": ref("timestamp"),
            "excluded_apps": {"type": "array", "items": obj(["app_id", "platform"], {
                "app_id": {"type": "string", "minLength": 1, "maxLength": 256},
                "platform": ref("platform"),
                "name": {"type": "string", "maxLength": 256},
                "builtin": {"type": "boolean"},
            })},
            "capture_kinds": {"type": "array", "uniqueItems": True, "items": ref("clipType")},
            "sensitive_handling": {"enum": ["drop", "store_masked", "store"]},
            "sensitive_categories_enabled": {"type": "array", "uniqueItems": True, "items": ref("sensitiveCategory")},
            "respect_os_markers": {"const": True, "description": "Mandatory. OS-concealed and Darkshot-excluded content is never captured."},
            "capture_primary_selection": {"type": "boolean", "description": "Linux only"},
        }),
        "if": {"properties": {"mode": {"const": "paused"}}},
        "then": {"required": ["paused_until"]},
    }),
    doc("ai-permission.schema.json", "AiPermission", obj(
        ["schema_version", "client_id", "display_name", "granted_at", "scopes", "include_sensitive"], {
            "schema_version": V1,
            "client_id": {"type": "string", "pattern": "^[a-z0-9][a-z0-9._-]{0,63}$"},
            "display_name": {"type": "string", "minLength": 1, "maxLength": 80},
            "granted_at": ref("timestamp"),
            "scopes": {"type": "array", "minItems": 1, "uniqueItems": True, "items": ref("scope")},
            "collection_filter": {"type": "array", "items": ref("uuid")},
            "max_age_days": {"type": "integer", "minimum": 1, "maximum": 365, "description": "Only clips newer than this are visible"},
            "include_sensitive": {"const": False},
            "expires_at": ref("timestamp"),
            "revoked_at": ref("timestamp"),
        }), "Grant for one AI/MCP client. Default: no grant at all. Sensitive clips are never grantable."),
    doc("operation-audit.schema.json", "OperationAudit", obj(
        ["schema_version", "id", "at", "actor", "operation", "result", "items"], {
            "schema_version": V1,
            "id": ref("uuid"),
            "at": ref("timestamp"),
            "actor": {"type": "string", "pattern": "^(ui|cli|mcp:[a-z0-9][a-z0-9._-]{0,63})$"},
            "operation": {"type": "string", "pattern": "^[a-z_]{3,64}$"},
            "result": {"enum": ["ok", "denied", "error"]},
            "items": {"type": "integer", "minimum": 0},
            "clip_ids": {"type": "array", "maxItems": 100, "items": ref("uuid")},
            "error_code": {"type": "string", "pattern": "^[A-Z][A-Z0-9_]{2,63}$"},
        }), "Content-free record of a CLI or MCP operation. No query text, no clip content."),
    doc("api-envelope.schema.json", "Application API / CLI JSON envelope", {
        **obj(["api_version", "ok"], {
            "api_version": V1,
            "ok": {"type": "boolean"},
            "data": {},
            "page": obj(["limit", "returned"], {
                "limit": {"type": "integer", "minimum": 1, "maximum": 200},
                "returned": {"type": "integer", "minimum": 0},
                "next_cursor": {"type": "string", "maxLength": 128},
            }),
            "error": obj(["code", "message"], {
                "code": {"enum": ["NOT_FOUND", "INVALID_ARGUMENT", "PERMISSION_DENIED", "UNSUPPORTED_PLATFORM", "CLIPBOARD_BUSY",
                                  "STORAGE_ERROR", "APP_NOT_RUNNING", "SCHEMA_TOO_NEW", "INTERNAL"]},
                "message": {"type": "string", "maxLength": 512, "description": "Never contains clip content"},
            }),
        }),
        "if": {"properties": {"ok": {"const": True}}},
        "then": {"required": ["data"], "not": {"required": ["error"]}},
        "else": {"required": ["error"], "not": {"required": ["data"]}},
    }, "Every CLI --json output and every Application API response serializes to this envelope."),
    doc("export.schema.json", "Collection export manifest", obj(
        ["schema_version", "app_version", "created_at", "collection", "clips", "blobs"], {
            "schema_version": V1,
            "app_version": {"type": "string", "pattern": "^\\d+\\.\\d+\\.\\d+"},
            "created_at": ref("timestamp"),
            "collection": {"$ref": "collection.schema.json"},
            "clips": {"type": "array", "items": {"$ref": "clip.schema.json"}},
            "blobs": {"type": "array", "items": obj(["sha256", "path", "size_bytes"], {
                "sha256": ref("sha256"),
                "path": {"type": "string", "pattern": "^blobs/[0-9a-f]{2}/[0-9a-f]{64}$"},
                "size_bytes": {"type": "integer", "minimum": 0},
            })},
        }), "Sensitive (masked) clips are never exported."),
]

OUT.mkdir(parents=True, exist_ok=True)
for name, d in schemas:
    (OUT / name).write_text(json.dumps(d, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"wrote {len(schemas)} schemas to {OUT}")
