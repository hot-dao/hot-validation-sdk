#!/usr/bin/env python3
# One-worker API interactor with per-endpoint JSON payloads (hardcoded).

import asyncio
import random
import time
from typing import Any, Callable, Dict, Optional

import httpx

# --- config (hardcoded) ---
BASE_URL = "http://localhost:8080"
INTERVAL_S  = 2.0     # base interval between cycles
JITTER_FRAC = 0.3     # ±30% jitter
READ_TIMEOUT = 10.0   # seconds

# Endpoints: method, path, optional json (dict or callable), optional headers.
# For GET, any json value is ignored.
ENDPOINTS = [
    # {"method": "GET",  "path": "/healthcheck"},

    # {"method": "POST", "path": "/sign", "json": """{"wallet_id":"72oZCoHAcHVkocLXM1KzQzDkZfDeAAucjtUU9Y8BUaib","key_type":0,"wallet_derive":"2rgKUfdGTErcyrYHso4ipyN6LRAqKTkqzP4LoNBQ3xsX","message_body":"25tZW79c9pWr64dr78zfFHyPxzo1XgJuTu5cHojUkUJB2177qrjX94uwyCkLBcEG5LooHTh4eBeKexrr5v66KhZg47ockDHzkq6BzgaSRVuun8sCt9NkEA9DuCxGsVKSaffpwEaKxFjLbzTaoVcZ1xe2mFmCfFbZa3FdJPYnfTXp6Dz51z7i9gZo5GeK1SsRdyQznYMTd1","message":"5HtRqWxt2mE2TvDMj4Td4YaLbXiWPy6PDzRRJNG62txB","user_payloads":["{\"auth_method\":0,\"signatures\":[\"4HvSdaVu8nbyDAJNnjx2ZDG1Wa3874o5cGnjH58JzYm4zJMyw4QcXe8BGcegizCbeQ17DeA147SLk1Tkqt1gJYX\"]}"],"auth_id":0,"key_gen":1,"receipt":{"signature":"2aHhmdsn7Pb4RUfnBkntkttoWRYjCgwJ8NjS76EMyWTrfW7HK49SHSDHcgrZgzup45ib1jM5F1tCCuDG5ZTh95o6","hash":"0x4c38957a48fd64d1656d73b55ba5603fc285d36a81ef235e74080f4f038e7b87","staker_id":"game.hot.tg","deadline":1759182450000}}"""},
    {
        "method": "POST",
        "path": "/sign",
        "json": {
            "wallet_id":"72oZCoHAcHVkocLXM1KzQzDkZfDeAAucjtUU9Y8BUaib",
            "key_type":0,
            "wallet_derive":"2rgKUfdGTErcyrYHso4ipyN6LRAqKTkqzP4LoNBQ3xsX",
            "message_body":"25tZW79c9pWr64dr78zfFHyPxzo1XgJuTu5cHojUkUJB2177qrjX94uwyCkLBcEG5LooHTh4eBeKexrr5v66KhZg47ockDHzkq6BzgaSRVuun8sCt9NkEA9DuCxGsVKSaffpwEaKxFjLbzTaoVcZ1xe2mFmCfFbZa3FdJPYnfTXp6Dz51z7i9gZo5GeK1SsRdyQznYMTd1",
            "message":"5HtRqWxt2mE2TvDMj4Td4YaLbXiWPy6PDzRRJNG62txB",
            "user_payloads":[
                "{\"auth_method\":0,\"signatures\":[\"4HvSdaVu8nbyDAJNnjx2ZDG1Wa3874o5cGnjH58JzYm4zJMyw4QcXe8BGcegizCbeQ17DeA147SLk1Tkqt1gJYX\"]}"
            ],
            "auth_id":0,
            "key_gen":1,
            "receipt":{
                "signature":"2aHhmdsn7Pb4RUfnBkntkttoWRYjCgwJ8NjS76EMyWTrfW7HK49SHSDHcgrZgzup45ib1jM5F1tCCuDG5ZTh95o6",
                "hash":"0x4c38957a48fd64d1656d73b55ba5603fc285d36a81ef235e74080f4f038e7b87",
                "staker_id":"game.hot.tg",
                "deadline":1759182450000
            }
        }
    },

    # Dynamic JSON example:
    # {"method": "POST", "path": "/sign", "json": lambda: {"message": "ping", "ts": int(time.time()*1000)}},
]
# --------------------------

def payload_for(ep: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    j = ep.get("json")
    if callable(j):
        return j()
    return j  # may be None or a dict

async def one_worker() -> None:
    limits = httpx.Limits(max_connections=10, max_keepalive_connections=10)
    timeout = httpx.Timeout(connect=5.0, read=READ_TIMEOUT, write=10.0, pool=None)

    async with httpx.AsyncClient(base_url=BASE_URL, limits=limits, timeout=timeout) as client:
        print("Starting one worker...")
        while True:
            async def do_req(ep: Dict[str, Any]):
                method = ep["method"].upper()
                path = ep["path"]
                headers = ep.get("headers")
                json_payload = payload_for(ep) if method != "GET" else None

                t0 = time.perf_counter()
                try:
                    r = await client.request(method, path, json=json_payload, headers=headers)
                    dt = time.perf_counter() - t0
                    ok = 200 <= r.status_code < 300
                    msg = f"{method} {path} -> {r.status_code} in {dt*1000:.1f}ms"
                    if not ok:
                        msg += f" | {r.text[:200]}"
                    print(msg)
                except Exception as e:
                    dt = time.perf_counter() - t0
                    print(f"{method} {path} -> ERR in {dt*1000:.1f}ms | {e}")

            # fire all hardcoded endpoints concurrently within this single worker cycle
            await asyncio.gather(*(do_req(ep) for ep in ENDPOINTS))

            # sleep with jitter
            jitter = random.uniform(-JITTER_FRAC, +JITTER_FRAC)
            await asyncio.sleep(max(0.0, INTERVAL_S * (1.0 + jitter)))

if __name__ == "__main__":
    try:
        asyncio.run(one_worker())
    except KeyboardInterrupt:
        pass
