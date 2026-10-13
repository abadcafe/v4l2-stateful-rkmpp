# 开发铁律（不可违反）

- 所有语言都要像 Rust 一样强类型，编译期解决所有类型错误，禁止忽略和跳过
- 任何代码修改后都必须全量验证（含格式化，检查，单测，集测等）

## 1. Python：100% 类型注解，0 编译警告和错误

技术栈: uv, Python >= 3.14, Pydantic v2, Pytest, 使用pyproject.toml

强类型约束:
- **所有 Python 代码（包括 `*_tests.py`）必须有完整的类型注解**
- 必须像 Rust 一样强类型，编译期解决所有类型错误，禁止忽略和跳过
- 必须使用 `type` 类型别名语法和泛型函数语法
- 禁止使用 `# pyright: ignore` 和 `# type: ignore` 等方式掩盖错误
- 优先使用 `Pydantic` 而不是 `cast`

禁止抛异常:
- 所有业务错误必须用 Result/Optional 机制处理（如 `Ok[T] | Error`），像 Rust 一样
- **禁止捕获** `AssertionError`、`IndexError`、`KeyError`、`TypeError`、`RuntimeError` 等编程错误，就让它崩溃暴露 bug
- 第三方库抛出的异常如果是正常流程控制（例如网络错误处理等），则必须在调用点尽量窄地捕获并正确处理, **其他情况禁止捕获**，就让它崩溃暴露 bug
- 测试代码中用 `assert isinstance(result, Ok)` / `assert isinstance(result, Error)` 替代 `pytest.raises`

各种验证的执行顺序:
1. `uv run ruff format`         # 全量格式化
2. `uv run ruff format --check` # 0 警告和错误
3. `uv run ruff check`          # 0 警告和错误
4. `uv run basedpyright`        # 0 警告和错误
5. `uv run pytest`              # 测试全部通过, 禁止跳过

## 2. Rust：0 编译警告和错误

各种验证的执行顺序:
1. `cargo +nightly fmt` # 全量格式化
2. `cargo clippy`       # 0 警告和错误
3. `cargo build`        # 0 警告和错误
4. `cargo test`         # 单测全部通过, 禁止跳过
5. `uv run pytest`      # 集测全部通过, 禁止跳过
