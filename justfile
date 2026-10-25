# 应用（Gloss）的命令入口
# 本地开发与持续集成共用同一套命令，保证行为一致；配方按用途分桶组织，
# 与脚本目录及工作流目录同构：本地开发与构建、需要编译的质量门禁、发布链、
# 提交前同款的纯文本检查。列出全部配方看默认目标。

# ---- 全局默认值 ----
set shell := ["bash", "-uc"]

# 默认目标：显示帮助
default:
    @just --list

# 行覆盖率下限：低于即失败（本地与持续集成共用同一判定）
coverage_min := "70"

# 门禁解释器入口：全部脚本配方经此变量调用，解释器升级只翻这一处
py := "python3.12"

# ---- 本地开发与构建 ----

# 运行开发版
run:
    cargo run

# 监听文件变化自动重编译运行，需要文件监听工具（cargo-watch）
watch:
    cargo watch -x run

# 打印日志目录
logs-dir:
    @echo "${HOME}/.gloss/logs"

# 跟随最新日志文件，按键退出
logs *args:
    @{{py}} scripts/dev/gloss-logs.py {{args}}

# 本地预览站点；同源缺更新清单时，页面走发布托管降级路径
site-preview port="8137":
    {{py}} -m http.server {{port}} --directory web

# 调试构建整个工作区
build:
    cargo build --workspace

# 指定目标三元组构建（持续集成构建矩阵用）
build-target target:
    cargo build --workspace --target {{target}}

# 发布优化构建
build-release:
    cargo build --release

# 清理构建产物
clean:
    cargo clean

# 列出依赖树
deps:
    cargo tree

# 克隆后一键环境初始化：工具链校验加可选工具清点加仓库钩子安装（幂等）
setup:
    {{py}} scripts/dev/setup_dev.py

# 重建应用图标产物：图标包与程序坞（Dock）图标图像（PNG）
icons:
    {{py}} scripts/dev/build_app_icon.py

# 跑核心库（gloss-core）热点基准（criterion）：自动对照上一次运行，并把结果存为本机滚动基线
bench:
    cargo bench --bench core -- --save-baseline last

# 把结果另存为命名基线（审计锚点，如在干净主分支上存同名基线），数据落在各基准目录下
bench-save name:
    cargo bench --bench core -- --save-baseline {{name}}

# 以命名基线为对照重跑基准（缺省取上一次运行；供汇总配方呈现对照数字，退出码不是门禁）
bench-check name="last":
    cargo bench --bench core -- --baseline {{name}}

# 汇总最近一次基准运行为标记语言（Markdown）表格（均值、95％区间、对照变化），标签仅用于展示
bench-summary baseline="last run":
    {{py}} scripts/perf/bench-summary.py "{{baseline}}"

# 跑克隆热点基准，基准框架（criterion）自定义测量：分配次数与字节，结果存滚动基线
clone-bench:
    cargo bench --bench clone -- --save-baseline last

# 打印克隆分布与热点分配（口径见脚本说明）
clone-stats:
    {{py}} scripts/perf/clone-stats.py

# 采集性能趋势点并重绘趋势图：聚合脚本把本地产物折算成历史一个点（克隆计数
# 与分配需先跑克隆基准、核心墙钟需先跑热点基准、自检信号需先跑自检报告、
# 应用片段需先跑应用度量；某源缺席则对应指标跳过），重绘脚本出三张趋势图；
# 检视时看历史数值与三张图的前后对照
perf-baseline:
    {{py}} scripts/perf/perf-collect.py
    {{py}} scripts/perf/perf-chart.py

# 仅重绘三张趋势图（SVG）：历史数据的派生视图，不采集不判罚
perf-chart:
    {{py}} scripts/perf/perf-chart.py

# 对照历史最新点的克隆门禁基线报告变化（克隆计数或热点分配上升即失败；
# 需先跑克隆基准与趋势采集；审计对照，不在提交前门禁内）
clone-check:
    {{py}} scripts/perf/clone-stats.py --check

# 量化应用运行时资源并采集趋势点：构建后拉起实例采启动里程碑、常驻内存（RSS）、处理器（CPU）
# 与体积（建议先退出在跑的应用），片段不入库；随聚合脚本折算进历史，
# 三张趋势图随重绘脚本更新
app-metrics:
    cargo build --release
    {{py}} scripts/perf/app-metrics.py
    {{py}} scripts/perf/perf-collect.py
    {{py}} scripts/perf/perf-chart.py

# 性能总览与对比报告，格式为标记语言（Markdown），打到标准输出、判定仅参考非门禁：缺省输出
# 最新点总览，给两期锚点则输出对比（精确计数类上升即回归，其余按注册表噪声阈值）
perf-report *args:
    {{py}} scripts/perf/perf-report.py {{args}}

# 以剖析配置构建并启动带符号实例（发布同级优化但保留符号表；先退出在跑的应用）
profile-run:
    cargo build --profile profiling
    ./target/profiling/gloss &

# 采运行中实例的处理器（CPU）火焰图（SVG）：提权跟踪采样（dtrace）加折叠渲染（inferno），
# 采样需管理员权限；输出到剖析目录，附原始栈与栈顶自耗排行
flame pid="" duration="10":
    {{py}} scripts/perf/flamegraph.py --pid "{{pid}}" --duration {{duration}}

# 从启动起采样出启动路径火焰图（只让采样器提权，应用仍以普通用户运行；
# 采样到应用退出为止，采完退出应用即出图）
flame-startup:
    mkdir -p target/profile
    cargo flamegraph --root --profile profiling --bin gloss --output target/profile/flamegraph-startup.svg

# 二进制体积归因（记录性不判罚）：节级尺寸统计加符号级地址差分
# （剥过符号的产物只做节级，符号级用剖析或调试产物）
size-report bin="target/release/gloss" top="30":
    {{py}} scripts/perf/size_report.py --bin {{bin}} --top {{top}}

# 提示词评测实跑轨：真实大语言模型（LLM），需专用凭据且显式选择参与
# 录制开关回写夹具；评分开关启用评分轨（每条额外一次模型调用）
eval *args:
    cargo run -p gloss-eval --bin eval -- live {{args}}

# 提示词评估检视：离线重放加单文件检视页（准确率、逐条轨迹、趋势），生成后开浏览器
# 改提示词后看真实准确率走实跑轨评测配方；本配方是免凭据的夹具口径
eval-inspect:
    cargo run -p gloss-eval --bin eval -- inspect
    open target/eval/inspect.html

# 提示词评测离线重放：夹具驱动，无网络无凭据，持续集成安全
eval-replay:
    cargo run -p gloss-eval --bin eval -- replay

# 把最近一次评测报告渲染为标记语言（Markdown）表格（参数可换目录）
eval-report *args:
    cargo run -p gloss-eval --bin eval -- report {{args}}

# ---- 需要编译的质量门禁（本地与持续集成共用同一配方）----

# 格式化检查
fmt:
    cargo fmt --all -- --check

# 自动格式化
fmt-fix:
    cargo fmt --all

# 配置格式（TOML）检查，检查工具（tombi）只检查不落盘
fmt-toml:
    tombi format --check $(git ls-files '*.toml')

# 配置格式（TOML）自动格式化，落盘
fmt-toml-fix:
    tombi format $(git ls-files '*.toml')

# 配置格式（TOML）语法与结构检查，错误级仍会失败
lint-toml:
    tombi lint $(git ls-files '*.toml')

# 脚本语言（Python）格式化检查，检查工具（ruff）只检查不落盘；显式传路径，点目录不被默认发现
fmt-py:
    ruff format --check scripts .githooks .zcode/hooks

# 脚本语言（Python）自动格式化，落盘
fmt-py-fix:
    ruff format scripts .githooks .zcode/hooks

# 脚本语言（Python）静态检查（ruff），规则见检查配置，零告警
lint-py:
    ruff check scripts .githooks .zcode/hooks

# 静态检查（Clippy）严格模式，警告即失败
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# 自动修复部分静态检查（Clippy）建议
lint-fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# 运行全部测试
test:
    cargo test --workspace --all-features

# 检查器单测：目录发现跑门禁脚本与客户端钩子目录的测试
hooks-test:
    {{py}} -m pytest scripts/hooks .zcode/hooks -q

# 第三层显隐自检（L3）：一百轮浮层显隐与首帧预算，需窗口服务与图形硬件（GPU）
selftest:
    cargo test -p gloss --test overlay

# 第三层启动预算自检（L3）：渲染栈初始化墙钟对预算常量，需窗口服务与图形硬件（GPU）
startup-selftest:
    cargo test -p gloss --test startup

# 第三层双自检包装（L3）：跑显隐与启动自检，从标准错误提取结构化信号行并外部轮询
# 完整常驻内存（RSS）曲线，落到指定输出（缺省在性能目录下，本地交接产物不入仓库）
selftest-report out="target/perf/selftest.jsonl":
    {{py}} scripts/perf/selftest_wrapper.py --out {{out}}

# 测试覆盖率：摘要加网页（HTML）报告，低于行覆盖下限即失败
coverage:
    cargo llvm-cov clean --workspace
    cargo llvm-cov --no-report --workspace --all-features
    cargo llvm-cov report --workspace --html
    cargo llvm-cov report --workspace --fail-under-lines {{coverage_min}}

# 测试覆盖率（仅终端摘要），按同一阈值判定
coverage-check:
    cargo llvm-cov --workspace --all-features --fail-under-lines {{coverage_min}}

# 安全审计，查已知漏洞库（RustSec）收录项
audit:
    cargo audit

# 依赖合规检查（许可证、重复依赖、来源）
deny:
    cargo deny check

# 内存未定义行为检测（Miri）：每夜工具链，只覆盖核心库（gloss-core）纯逻辑层
miri:
    MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test -p gloss-core --all-features -- --skip cache:: --skip engine:: --skip file_writer_persists_lines_into_daily_file

# 完整质量门禁：提交前门禁加检查器单测与全量测试
check: precommit hooks-test test
    @echo "ok: all quality gates passed"

# 提交前门禁：纯文本门禁组合加静态检查（Clippy）
precommit: text-gates lint
    @echo "ok: precommit checks passed (tests left to CI)"

# ---- 发布链：持续集成（CI）的发布工作流用同一批脚本 ----

# 发布打包：发布构建、打包出应用包、临时签名、压缩为分发镜像
package-macos: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    cargo bundle --release --format osx
    app="target/release/bundle/osx/Gloss.app"
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
    {{py}} scripts/release/bundle_dmg.py "$app"
    echo "artifacts: $app and ${app%.app}.dmg"

# 发布冒烟：启动应用包，验证能启动、活得住、日志无崩溃
smoke-app app:
    {{py}} scripts/release/smoke_app.py {{app}}

# 校验发布标签与版本单点一致
release-check tag:
    {{py}} scripts/release/check_release_tag.py {{tag}}

# 本地生成站点更新清单打到标准输出（产物目录须含双架构的压缩包与镜像共四个产物）
gen-manifest tag artifacts-dir:
    {{py}} scripts/release/gen_manifest.py {{tag}} {{artifacts-dir}}

# ---- 纯文本快速检查（提交前门禁同款，秒级；持续集成的钩子工作流也跑）----

# 纯文本门禁组合：密钥扫描前置，再跑文档引用、行为清单（BDD）、约束、文案表、注释纪律
# 与五条工具格式检查；本语言格式化只解析不编译，归文本层
text-gates: secrets agents-doc test-bdd constraints i18n comments fmt fmt-py lint-py fmt-toml lint-toml

# 硬编码密钥扫描（命中即失败）
secrets:
    {{py}} scripts/hooks/check_secrets.py

# 校验项目文档引用的配方、路径、测试目标与质量条目名真实存在
agents-doc:
    {{py}} scripts/hooks/check_agents_doc.py

# 测试行为清单与测试源码双向核对（缺登记或失同步即失败）
test-bdd:
    {{py}} scripts/hooks/check_test_bdd.py

# 自动化门禁：依赖方向、日志、界面库（egui）上下文装入点、版本单点、依赖特性、残留标记
constraints:
    {{py}} scripts/hooks/check_constraints.py

# 界面文案统一进文案表（文案字面量、装入点、词条表唯一）
i18n:
    {{py}} scripts/hooks/check_i18n.py

# 注释纪律：源码（Rust）只留中文文件头注释（不变量注释与放行标记豁免，其余迁
# 记录文件）；注释主体全中文，专名只进全角括号注记，不变量注释纯英文
comments:
    {{py}} scripts/hooks/comments/check_comments.py

# 校验单条提交信息是否符合提交规范
lint-commit file:
    {{py}} scripts/hooks/check_commit_msg.py "{{file}}"

# 安装本地仓库钩子（克隆后运行一次）
install-hooks:
    {{py}} scripts/hooks/install_hooks.py
