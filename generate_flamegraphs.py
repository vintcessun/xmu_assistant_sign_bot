import subprocess
from pathlib import Path


def run_flamegraph_all():
    # 1. 确保在 Cargo 项目根目录
    if not Path("Cargo.toml").exists():
        print("错误：请在 Rust 项目根目录下运行此脚本。")
        return

    # 2. 识别所有的 bench 目标
    # 方式：扫描 benches/ 目录下的 .rs 文件
    bench_dir = Path("benches")
    if not bench_dir.exists():
        print("未找到 benches 目录。")
        return

    bench_targets = [f.stem for f in bench_dir.glob("*.rs")]

    # 3. 创建输出目录
    output_dir = Path("flamegraphs_output")
    output_dir.mkdir(exist_ok=True)

    print(f"检测到 {len(bench_targets)} 个测试目标: {bench_targets}")

    for target in bench_targets:
        print(f"\n[正在分析] 目标: {target} ...")

        svg_name = output_dir / f"flamegraph_{target}.svg"

        # 构建命令: cargo flamegraph --bench <target> -o <output_path> -- --bench
        # 注意: 最后的 -- --bench 是传递给 criterion 等框架的，确保其以 bench 模式运行
        cmd = [
            "cargo",
            "flamegraph",
            "--bench",
            target,
            "-o",
            str(svg_name),
            "--",
            "--bench",
        ]

        try:
            # 运行命令并实时显示输出
            subprocess.run(cmd, check=True)
            print(f"成功生成: {svg_name}")
        except subprocess.CalledProcessError as e:
            print(f"分析失败: {target}。错误信息: {e}")


if __name__ == "__main__":
    run_flamegraph_all()
