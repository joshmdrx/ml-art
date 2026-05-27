"""Tests for ml_art.vectors."""

from __future__ import annotations

import math

import numpy as np
import pytest

from ml_art.vectors import cosine, mean_vector, normalize, top_k


def test_normalize_unit_vector_unchanged():
    v = np.array([1.0, 0.0, 0.0], dtype=np.float32)
    np.testing.assert_allclose(normalize(v), v, atol=1e-6)


def test_normalize_makes_unit_length():
    v = np.array([3.0, 4.0], dtype=np.float32)
    out = normalize(v)
    assert math.isclose(np.linalg.norm(out), 1.0, abs_tol=1e-6)


def test_normalize_batch():
    vs = np.array([[3.0, 4.0], [1.0, 0.0], [0.0, 2.0]], dtype=np.float32)
    out = normalize(vs)
    norms = np.linalg.norm(out, axis=1)
    np.testing.assert_allclose(norms, np.ones(3), atol=1e-6)


def test_normalize_zero_vector_does_not_explode():
    v = np.zeros(4, dtype=np.float32)
    out = normalize(v)
    assert np.all(np.isfinite(out))


def test_cosine_identical_is_one():
    v = np.array([1.0, 2.0, 3.0], dtype=np.float32)
    assert math.isclose(cosine(v, v), 1.0, abs_tol=1e-6)


def test_cosine_orthogonal_is_zero():
    a = np.array([1.0, 0.0], dtype=np.float32)
    b = np.array([0.0, 1.0], dtype=np.float32)
    assert math.isclose(cosine(a, b), 0.0, abs_tol=1e-6)


def test_cosine_opposite_is_minus_one():
    a = np.array([1.0, 0.0], dtype=np.float32)
    b = np.array([-1.0, 0.0], dtype=np.float32)
    assert math.isclose(cosine(a, b), -1.0, abs_tol=1e-6)


def test_cosine_query_vs_batch():
    q = np.array([1.0, 0.0], dtype=np.float32)
    corpus = np.array([[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], dtype=np.float32)
    sims = cosine(q, corpus)
    np.testing.assert_allclose(sims, [1.0, 0.0, -1.0], atol=1e-6)


def test_top_k_basic():
    q = np.array([1.0, 0.0], dtype=np.float32)
    corpus = np.array(
        [[1.0, 0.0], [0.9, 0.1], [0.0, 1.0], [-1.0, 0.0]],
        dtype=np.float32,
    )
    results = top_k(q, corpus, k=2)
    assert [i for i, _ in results] == [0, 1]
    assert results[0][1] > results[1][1]


def test_top_k_respects_exclude():
    q = np.array([1.0, 0.0], dtype=np.float32)
    corpus = np.array(
        [[1.0, 0.0], [0.9, 0.1], [0.0, 1.0]],
        dtype=np.float32,
    )
    results = top_k(q, corpus, k=2, exclude={0})
    indices = [i for i, _ in results]
    assert 0 not in indices
    assert indices[0] == 1


def test_top_k_k_larger_than_corpus():
    q = np.array([1.0, 0.0], dtype=np.float32)
    corpus = np.array([[1.0, 0.0], [0.0, 1.0]], dtype=np.float32)
    results = top_k(q, corpus, k=10)
    assert len(results) == 2


def test_mean_vector_shape_and_value():
    vs = np.array([[1.0, 2.0], [3.0, 4.0]], dtype=np.float32)
    m = mean_vector(vs)
    np.testing.assert_allclose(m, [2.0, 3.0], atol=1e-6)
    assert m.shape == (2,)


def test_mean_vector_rejects_1d():
    with pytest.raises(ValueError):
        mean_vector(np.array([1.0, 2.0], dtype=np.float32))
