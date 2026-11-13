"""Armazém vetorial em memória (casca).

Trocar por pgvector/Qdrant mantendo a interface de `Store`. O "embedding" daqui é um
hash de palavras: NÃO é semântico, só faz o fluxo ingest -> query funcionar sem custo.
Troque `embed` por um modelo real (sentence-transformers local, ou via ai-gateway).
"""
from __future__ import annotations

import hashlib
import math
from dataclasses import dataclass
from typing import List, Tuple

DIM = 256


def embed(text: str) -> List[float]:
    v = [0.0] * DIM
    for w in text.lower().split():
        h = int(hashlib.sha256(w.encode()).hexdigest()[:8], 16)
        v[h % DIM] += 1.0
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / n for x in v]


def chunk(text: str, size: int = 500, overlap: int = 50) -> List[str]:
    out, i = [], 0
    while i < len(text):
        out.append(text[i : i + size])
        i += size - overlap
    return out


@dataclass
class Hit:
    doc_id: str
    text: str
    score: float


class Store:
    def __init__(self) -> None:
        self._rows: List[Tuple[str, str, List[float]]] = []

    def add(self, doc_id: str, text: str) -> int:
        parts = chunk(text)
        for p in parts:
            self._rows.append((doc_id, p, embed(p)))
        return len(parts)

    def search(self, query: str, k: int) -> List[Hit]:
        q = embed(query)
        scored = [Hit(d, t, sum(a * b for a, b in zip(q, v))) for d, t, v in self._rows]
        scored.sort(key=lambda h: h.score, reverse=True)
        return scored[:k]

    def __len__(self) -> int:
        return len(self._rows)
