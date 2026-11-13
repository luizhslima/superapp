from fastapi.testclient import TestClient

from app.main import app

c = TestClient(app)


def test_ingest_and_query_finds_relevant_doc():
    c.post("/v1/ingest", json={"doc_id": "a", "text": "fechamento financeiro leva dias"})
    c.post("/v1/ingest", json={"doc_id": "b", "text": "receita de bolo de cenoura"})
    r = c.post("/v1/query", json={"question": "fechamento financeiro", "k": 1})
    assert r.status_code == 200
    assert r.json()["passages"][0]["doc_id"] == "a"


def test_limits():
    assert c.post("/v1/ingest", json={"doc_id": "", "text": "x"}).status_code == 422
    assert c.post("/v1/query", json={"question": "x", "k": 99}).status_code == 422
