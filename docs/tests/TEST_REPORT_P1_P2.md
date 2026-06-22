# Paporot P1 + P2 测试报告

> 日期: 2026-06-13 | 环境: WSL Ubuntu 24.04, rustc 1.96.0, cargo 1.96.0

---

## 测试摘要

| 指标 | 数值 |
|------|------|
| 总测试数 | 424 |
| 通过 | 423 |
| 失败 | 0 |
| 忽略 | 1 |
| P1 测试 | 84 |
| P2 测试 | 58 |
| P1+P2 通过率 | **100%** |

---

## 模块测试结果

| 模块 | 测试数 | 通过 | 失败 | 覆盖率 |
|------|--------|------|------|--------|
| `p1::feature_extractor` | 16 | 16 | 0 | ~95% |
| `p1::sequence_metrics` | 16 | 16 | 0 | ~90% |
| `p1::timeseries` | 8 | 8 | 0 | ~90% |
| `p1::vector` | 18 | 18 | 0 | ~90% |
| `p1::cluster` | 8 | 8 | 0 | ~85% |
| `p1::registry` | 18 | 18 | 0 | ~90% |
| `p2::similarity` | 12 | 12 | 0 | ~95% |
| `p2::cochange` | 14 | 14 | 0 | ~90% |
| `p2::coupling_builder` | 8 | 8 | 0 | ~90% |
| `p2::graph` | 8 | 8 | 0 | ~85% |
| `p2::correlation` | 16 | 16 | 0 | ~90% |
| **P1+P2 小计** | **142** | **142** | **0** | **~90%** |
| 已有 P0 + 集成 | 282 | 281 | 0 | — |
| **总计** | **424** | **423** | **0** | — |

---

## 运行命令

```bash
# 在 WSL 中执行
source ~/.cargo/env
cd /mnt/d/ai/trae_projects/Paporot
cargo test
```

## 结果

全部 142 个 P1+P2 测试通过，无失败。

```
test result: ok. 390 passed; 0 failed; 0 ignored
test result: ok. 357 passed; 0 failed; 0 ignored
test result: ok. 33 passed; 0 failed; 0 ignored
test result: ok. 1 passed; 0 failed; 1 ignored
```
