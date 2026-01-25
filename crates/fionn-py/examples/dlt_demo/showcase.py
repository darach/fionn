import json
import subprocess
import sys
from pathlib import Path

import pandas as pd
import plotly.express as px
import plotly.graph_objects as go
import streamlit as st

st.set_page_config(
    page_title="Fionn Performance Explorer", layout="wide", initial_sidebar_state="expanded"
)

# Override Streamlit's default red accent colors with blue/green
st.markdown(
    """
<style>
    /* Primary button - green instead of red */
    .stButton > button[kind="primary"] {
        background-color: #00CC96 !important;
        border-color: #00CC96 !important;
    }
    .stButton > button[kind="primary"]:hover {
        background-color: #00B386 !important;
        border-color: #00B386 !important;
    }

    /* Tab styling - remove red underline */
    .stTabs [data-baseweb="tab-list"] button[aria-selected="true"] {
        border-bottom-color: #00CC96 !important;
    }
    .stTabs [data-baseweb="tab-highlight"] {
        background-color: #00CC96 !important;
    }

    /* Multiselect tags */
    .stMultiSelect [data-baseweb="tag"] {
        background-color: #636EFA !important;
    }

    /* Checkbox */
    .stCheckbox > label > div[data-testid="stMarkdownContainer"] + div > div {
        border-color: #00CC96 !important;
    }
    input[type="checkbox"]:checked + div {
        background-color: #00CC96 !important;
    }

    /* Selectbox focus */
    .stSelectbox [data-baseweb="select"] > div:focus-within {
        border-color: #00CC96 !important;
    }

    /* Spinner */
    .stSpinner > div > div {
        border-top-color: #00CC96 !important;
    }

    /* Links */
    a {
        color: #636EFA !important;
    }

    /* Metric positive delta - use green not red */
    [data-testid="stMetricDelta"] svg {
        fill: #00CC96 !important;
    }

    /* Sidebar header styling */
    .css-1d391kg, .css-1v0mbdj {
        border-color: #00CC96 !important;
    }
</style>
""",
    unsafe_allow_html=True,
)

# --- Color Schemes (blues/greens/purples - NO RED) ---
IMPL_COLORS = {
    "fionn (Rust)": "#2CA02C",  # Dark Green - Pure Rust (fastest)
    "fionn-py": "#00CC96",  # Light Green - Rust via PyO3
    "stdlib": "#636EFA",  # Blue - Python standard library
    "orjson": "#AB63FA",  # Purple - Rust binding
    "ujson": "#19D3F3",  # Cyan - C binding
    "simdjson": "#FFA15A",  # Orange - C++ SIMD binding
}

CATEGORY_COLORS = {
    "Full Parse (DLT)": "#636EFA",  # Blue
    "Selective Parse": "#00CC96",  # Green
    "Format Translation": "#AB63FA",  # Purple
    "Transformation": "#19D3F3",  # Cyan
    "CRDT Merge": "#FFA15A",  # Orange
    "Conflict Resolution": "#B6E880",  # Light green
    "Row Filtering": "#FF97FF",  # Pink (not red!)
    "Memory Pattern": "#FECB52",  # Yellow
    "Allocation": "#72B7B2",  # Teal
    "Parsing": "#636EFA",  # Blue (default)
}

FORMAT_COLORS = {
    "JSONL": "#636EFA",  # Blue
    "ISONL": "#00CC96",  # Green
    "CSV": "#AB63FA",  # Purple
}

OPERATION_COLORS = {
    "Full JSONL Parse": "#636EFA",
    "Full ISONL Parse": "#00CC96",
    "Selective Parse": "#AB63FA",
    "Format Translation": "#19D3F3",
    "Flatten": "#FFA15A",
    "Merge": "#B6E880",
    "Conflict Resolution": "#FF97FF",
    "Row Filtering": "#FECB52",
    "Allocation": "#72B7B2",
    "Memory Load": "#636EFA",  # Blue
    "Streaming": "#00CC96",
    "Parse": "#636EFA",
}

PARALLELISM_COLORS = {
    "Single": "#636EFA",
    "Multi-core": "#00CC96",
}

BOTTLENECK_COLORS = {
    "DLT/DuckDB": "#636EFA",  # Blue - external bottleneck
    "Parsing": "#00CC96",  # Green - fionn can win here
    "CPU": "#AB63FA",  # Purple
    "CPU + Threading": "#19D3F3",  # Cyan
    "I/O + Parsing": "#FFA15A",  # Orange
    "Memory": "#B6E880",  # Light green
    "Allocator": "#72B7B2",  # Teal
}


def get_color_map(color_by: str) -> dict:
    """Get the appropriate color map for a given facet."""
    return {
        "implementation": IMPL_COLORS,
        "category": CATEGORY_COLORS,
        "format": FORMAT_COLORS,
        "operation": OPERATION_COLORS,
        "parallelism": PARALLELISM_COLORS,
        "bottleneck": BOTTLENECK_COLORS,
    }.get(color_by, IMPL_COLORS)


# Set default plotly color sequence (NO RED) and template
NO_RED_COLORS = [
    "#2CA02C",
    "#00CC96",
    "#636EFA",
    "#AB63FA",
    "#19D3F3",
    "#FFA15A",
    "#B6E880",
    "#FF97FF",
    "#FECB52",
    "#72B7B2",
]
px.defaults.color_discrete_sequence = NO_RED_COLORS

# Create custom template that removes all red
import plotly.io as pio

pio.templates["no_red"] = go.layout.Template(
    layout=go.Layout(colorway=NO_RED_COLORS, font={"color": "#333"})
)
pio.templates.default = "plotly+no_red"


# --- Smart Unit Formatting ---
def format_bytes_rate(bytes_per_sec: float) -> tuple[float, str]:
    """Auto-scale to appropriate unit (B/s, KiB/s, MiB/s, GiB/s)."""
    if bytes_per_sec <= 0:
        return 0.0, "B/s"
    units = [("B/s", 1), ("KiB/s", 1024), ("MiB/s", 1024**2), ("GiB/s", 1024**3)]
    for unit, divisor in reversed(units):
        if bytes_per_sec >= divisor:
            return bytes_per_sec / divisor, unit
    return bytes_per_sec, "B/s"


def format_time(seconds: float) -> tuple[float, str]:
    """Auto-scale time to appropriate unit."""
    if seconds < 0.001:
        return seconds * 1_000_000, "μs"
    elif seconds < 1:
        return seconds * 1000, "ms"
    elif seconds < 60:
        return seconds, "s"
    else:
        return seconds / 60, "min"


def format_memory(mb: float) -> tuple[float, str]:
    """Auto-scale memory to appropriate unit."""
    if mb < 1:
        return mb * 1024, "KiB"
    elif mb < 1024:
        return mb, "MiB"
    else:
        return mb / 1024, "GiB"


# --- Data File Mapping (only for actual I/O throughput benchmarks) ---
DATA_DIR = Path("data")
FILE_SIZE_MAP = {
    # DLT Pipeline benchmarks (all parsers)
    "Baseline (Full JSONL)": DATA_DIR / "events_wide.jsonl",
    "orjson (Full JSONL)": DATA_DIR / "events_wide.jsonl",
    "ujson (Full JSONL)": DATA_DIR / "events_wide.jsonl",
    "simdjson (Full JSONL)": DATA_DIR / "events_wide.jsonl",
    "fionn-py (Full JSONL)": DATA_DIR / "events_wide.jsonl",
    "fionn-py Selective (3 fields)": DATA_DIR / "events_wide.jsonl",
    "fionn-py ISONL (3 fields)": DATA_DIR / "events_wide.isonl",
    "fionn-py CSV->ISONL": DATA_DIR / "events.csv",
    "fionn-py ISONL SIMD": DATA_DIR / "events_wide.isonl",
    # Pure Rust CLI (no Python overhead)
    "fionn (Rust) stream": DATA_DIR / "events_wide.jsonl",
    "fionn (Rust) selective": DATA_DIR / "events_wide.jsonl",
    "fionn (Rust) CSV->JSONL": DATA_DIR / "events.csv",
    # Transformation benchmarks
    "Baseline (Python Flatten)": DATA_DIR / "events_nested.jsonl",
    "Semantic (Fionn GRON)": DATA_DIR / "events_nested.jsonl",
    "fionn (Rust) gron": DATA_DIR / "events_nested.jsonl",
    # Merge benchmarks
    "Baseline (Python Merge)": DATA_DIR / "profiles_a.jsonl",
    "Semantic (Fionn CRDT)": DATA_DIR / "profiles_a.jsonl",
    "Semantic (Parallel CRDT)": DATA_DIR / "profiles_a.jsonl",
    "fionn (Rust) merge": DATA_DIR / "profiles_a.jsonl",
    # Conflict resolution
    "Baseline (Python Storm)": DATA_DIR / "storm_updates.jsonl",
    "Semantic (Fionn Storm)": DATA_DIR / "storm_updates.jsonl",
    "Semantic (Native Storm)": DATA_DIR / "storm_updates.jsonl",
    "fionn (Rust) stream stats": DATA_DIR / "storm_updates.jsonl",
    # Filter benchmarks
    "Baseline (Python Filter)": DATA_DIR / "events_wide.jsonl",
    "Semantic (Fionn Filter)": DATA_DIR / "events_wide.jsonl",
    # NOTE: Memory/Pooling benchmarks excluded - they don't measure I/O throughput
}

# --- Clean Name Mapping ---
# stdlib = Python standard library
# orjson/ujson/simdjson = C/C++/Rust Python bindings
# fionn-py = Rust via PyO3 (has Python overhead)
# fionn (Rust) = Pure Rust CLI (no Python overhead)
NAME_MAP = {
    # DLT Pipeline - Full file parsing (DLT bottlenecked)
    "Baseline (Full JSONL)": "stdlib json.loads() + DLT",
    "orjson (Full JSONL)": "orjson [Rust] + DLT",
    "ujson (Full JSONL)": "ujson [C] + DLT",
    "simdjson (Full JSONL)": "pysimdjson [C++] + DLT",
    "fionn-py (Full JSONL)": "fionn-py + DLT",
    "fionn-py Selective (3 fields)": "fionn-py selective + DLT",
    "fionn-py ISONL (3 fields)": "fionn-py ISONL + DLT",
    "fionn-py CSV->ISONL": "fionn-py CSV→ISONL + DLT",
    "fionn-py ISONL SIMD": "fionn-py ISONL SIMD + DLT",
    # Pure Rust (no Python, no DLT)
    "fionn (Rust) stream": "fionn (Rust) stream",
    "fionn (Rust) selective": "fionn (Rust) selective",
    "fionn (Rust) CSV->JSONL": "fionn (Rust) CSV→JSONL",
    # Transformation
    "Baseline (Python Flatten)": "stdlib recursive flatten",
    "Semantic (Fionn GRON)": "fionn-py gron()",
    "fionn (Rust) gron": "fionn (Rust) gron",
    # Merge
    "Baseline (Python Merge)": "stdlib dict.update()",
    "Semantic (Fionn CRDT)": "fionn-py crdt_merge()",
    "Semantic (Parallel CRDT)": "fionn-py crdt 4T",
    "fionn (Rust) merge": "fionn (Rust) merge",
    # Conflict resolution
    "Baseline (Python Storm)": "stdlib threading+dict",
    "Semantic (Fionn Storm)": "fionn-py conflict resolver",
    "Semantic (Native Storm)": "fionn-py stream resolver",
    "fionn (Rust) stream stats": "fionn (Rust) stream stats",
    # Row filtering
    "Baseline (Python Filter)": "stdlib list comprehension",
    "Semantic (Fionn Filter)": "fionn-py filter()",
    # Memory patterns
    "Baseline (DOM Memory)": "stdlib json.load() DOM",
    "Semantic (Fionn Stream)": "fionn-py streaming iter",
    # Allocation
    "Baseline (No Pooling)": "stdlib fresh allocs",
    "Semantic (Tape Pooling)": "fionn-py TapePool",
}


def clean_name(name: str) -> str:
    return NAME_MAP.get(name, name)


# --- Dimension Classification ---
def classify_benchmark(name: str) -> dict:
    """Classify benchmark into multiple dimensions for faceted analysis."""
    result = {
        "implementation": "stdlib",  # Default to stdlib if not matched
        "category": "Parsing",  # Default category
        "format": "JSONL",  # Default format
        "operation": "Parse",  # Default operation
        "parallelism": "Single",
        "bottleneck": "DLT/DuckDB",  # Most benchmarks are DLT-bottlenecked
    }

    # Implementation - be explicit about each parser
    if "fionn (Rust)" in name:
        result["implementation"] = "fionn (Rust)"  # Pure Rust CLI, no Python
        result["bottleneck"] = "Parsing"  # No DLT overhead
    elif "fionn-py" in name:
        result["implementation"] = "fionn-py"  # Rust via PyO3
    elif "Baseline" in name or name.startswith("Baseline"):
        result["implementation"] = "stdlib"
    elif "orjson" in name:
        result["implementation"] = "orjson"
    elif "ujson" in name:
        result["implementation"] = "ujson"
    elif "simdjson" in name:
        result["implementation"] = "simdjson"
    elif any(x in name for x in ["Fionn", "Native", "Semantic", "Ultra", "Selective", "Agile"]):
        result["implementation"] = "fionn-py"

    # Category
    if any(x in name for x in ["Full JSONL", "Full ISONL"]):
        result["category"] = "Full Parse (DLT)"
    elif "Selective" in name or "selective" in name:
        result["category"] = "Selective Parse"
        result["bottleneck"] = "Parsing"  # Fionn advantage here
    elif "Agile" in name or "CSV" in name:
        result["category"] = "Format Translation"
        result["bottleneck"] = "Parsing"  # Fionn advantage here
    elif any(x in name.lower() for x in ["flatten", "gron"]):
        result["category"] = "Transformation"
        result["bottleneck"] = "CPU"
    elif any(x in name.lower() for x in ["merge", "crdt"]):
        result["category"] = "CRDT Merge"
        result["bottleneck"] = "CPU"
    elif "storm" in name.lower() or "stream stats" in name.lower():
        result["category"] = "Conflict Resolution"
        result["bottleneck"] = "CPU + Threading"
    elif "Filter" in name:
        result["category"] = "Row Filtering"
        result["bottleneck"] = "I/O + Parsing"
    elif any(x in name for x in ["Memory", "DOM", "Stream"]):
        result["category"] = "Memory Pattern"
        result["bottleneck"] = "Memory"
    elif "Pooling" in name:
        result["category"] = "Allocation"
        result["bottleneck"] = "Allocator"

    # Format
    if "JSONL" in name:
        result["format"] = "JSONL"
    elif "ISONL" in name:
        result["format"] = "ISONL"
    elif "CSV" in name:
        result["format"] = "CSV"

    # Operation type
    if "Full" in name and "JSONL" in name:
        result["operation"] = "Full JSONL Parse"
    elif "Full" in name and "ISONL" in name:
        result["operation"] = "Full ISONL Parse"
    elif "Selective" in name:
        result["operation"] = "Selective Parse"
    elif "Agile" in name:
        result["operation"] = "Format Translation"
    elif "Flatten" in name or "GRON" in name:
        result["operation"] = "Flatten"
    elif "Merge" in name or "CRDT" in name:
        result["operation"] = "Merge"
    elif "Storm" in name:
        result["operation"] = "Conflict Resolution"
    elif "Filter" in name:
        result["operation"] = "Row Filtering"
    elif "Pooling" in name:
        result["operation"] = "Allocation"
    elif "DOM" in name or "Memory" in name:
        result["operation"] = "Memory Load"
    elif "Stream" in name:
        result["operation"] = "Streaming"

    # Parallelism
    if "Parallel" in name or ("Storm" in name and "Baseline" not in name):
        result["parallelism"] = "Multi-core"

    return result


def get_file_size_bytes(name: str) -> int:
    if name in FILE_SIZE_MAP:
        path = FILE_SIZE_MAP[name]
        if path.exists():
            return path.stat().st_size
    return 0


# --- Load and Enrich Data ---
@st.cache_data
def load_benchmark_data():
    results_file = Path("results.json")
    if not results_file.exists():
        return None

    with open(results_file) as f:
        data = json.load(f)

    df = pd.DataFrame(data["runs"])

    # Clean up names
    df["display_name"] = df["name"].apply(clean_name)

    # Enrich with classifications
    classifications = df["name"].apply(classify_benchmark).apply(pd.Series)
    df = pd.concat([df, classifications], axis=1)

    # Calculate derived metrics
    df["file_size_bytes"] = df["name"].apply(get_file_size_bytes)
    df["bytes_per_sec"] = df.apply(
        lambda r: r["file_size_bytes"] / r["duration"]
        if r["duration"] > 0 and r["file_size_bytes"] > 0
        else 0,
        axis=1,
    )

    # Smart formatted columns
    df["throughput_val"], df["throughput_unit"] = zip(*df["bytes_per_sec"].apply(format_bytes_rate))
    df["time_val"], df["time_unit"] = zip(*df["duration"].apply(format_time))
    df["mem_val"], df["mem_unit"] = zip(*df["memory_mb"].apply(format_memory))

    # Calculate baseline speedups per category (vs stdlib baseline in same category)
    def calc_speedup(row, df):
        cat_baseline = df[(df["category"] == row["category"]) & (df["implementation"] == "stdlib")]
        if len(cat_baseline) > 0:
            return cat_baseline["duration"].iloc[0] / row["duration"]
        # Fallback to first stdlib baseline
        baselines = df[df["implementation"] == "stdlib"]
        if len(baselines) > 0:
            return baselines["duration"].iloc[0] / row["duration"]
        return 1.0

    df["speedup"] = df.apply(lambda r: calc_speedup(r, df), axis=1)

    # Flag where fionn wins vs loses
    df["fionn_wins"] = (df["implementation"] == "fionn-py") & (df["speedup"] > 1.0)

    # Efficiency score (speedup per MB of memory)
    df["efficiency"] = df["speedup"] / (df["memory_mb"] / 100)

    return df, data.get("num_records", 50000)


# --- Sidebar Controls ---
st.sidebar.title("🎛️ Performance Explorer")

results_file = Path("results.json")

if st.sidebar.button("🔥 Run Benchmarks", type="primary"):
    with st.spinner("Running benchmarks..."):
        subprocess.run([sys.executable, "benchmark.py"], check=True)
    st.cache_data.clear()
    st.rerun()

data_result = load_benchmark_data()

if data_result is None:
    st.warning("No benchmark data found. Click 'Run Benchmarks' to generate data.")
    st.stop()

df, num_records = data_result

st.sidebar.divider()
st.sidebar.subheader("🔍 Facet Filters")

# Implementation filter
implementations = st.sidebar.multiselect(
    "Implementation", options=df["implementation"].unique(), default=df["implementation"].unique()
)

# Category filter
categories = st.sidebar.multiselect(
    "Category", options=df["category"].unique(), default=df["category"].unique()
)

# Format filter
formats = st.sidebar.multiselect(
    "Format", options=df["format"].unique(), default=df["format"].unique()
)

# Apply filters
mask = (
    df["implementation"].isin(implementations)
    & df["category"].isin(categories)
    & df["format"].isin(formats)
)
filtered_df = df[mask].copy()

st.sidebar.divider()
st.sidebar.subheader("📊 Visualization Options")

color_by = st.sidebar.selectbox(
    "Color By",
    ["implementation", "category", "format", "operation", "parallelism", "bottleneck"],
    index=0,
)

log_scale = st.sidebar.checkbox("Log Scale (Time)", value=False)
show_annotations = st.sidebar.checkbox("Show Value Labels", value=True)

st.sidebar.divider()
st.sidebar.caption(f"**Dataset:** {num_records:,} records")
st.sidebar.caption(f"**Benchmarks:** {len(filtered_df)} of {len(df)}")

# --- Main Content ---
st.title("🚀 Fionn Performance Explorer")
st.markdown("**Interactive dimensional analysis of Fionn (Rust) vs Python baseline performance**")

# --- KPI Summary Cards ---
st.header("📈 Performance Summary")

col1, col2, col3, col4, col5 = st.columns(5)

# Best throughput
best_throughput = (
    filtered_df.loc[filtered_df["bytes_per_sec"].idxmax()] if len(filtered_df) > 0 else None
)
if best_throughput is not None and best_throughput["bytes_per_sec"] > 0:
    val, unit = format_bytes_rate(best_throughput["bytes_per_sec"])
    col1.metric("🏆 Peak Throughput", f"{val:.2f} {unit}", best_throughput["display_name"][:25])

# Best speedup
best_speedup = filtered_df.loc[filtered_df["speedup"].idxmax()] if len(filtered_df) > 0 else None
if best_speedup is not None:
    col2.metric(
        "⚡ Max Speedup", f"{best_speedup['speedup']:.2f}x", best_speedup["display_name"][:25]
    )

# Lowest memory
lowest_mem = filtered_df.loc[filtered_df["memory_mb"].idxmin()] if len(filtered_df) > 0 else None
if lowest_mem is not None:
    val, unit = format_memory(lowest_mem["memory_mb"])
    col3.metric("💾 Min Memory", f"{val:.2f} {unit}", lowest_mem["display_name"][:25])

# Fionn vs stdlib aggregate
fionn_df = filtered_df[filtered_df["implementation"] == "fionn-py"]
python_df = filtered_df[filtered_df["implementation"] == "stdlib"]
if len(fionn_df) > 0 and len(python_df) > 0:
    avg_fionn_time = fionn_df["duration"].mean()
    avg_python_time = python_df["duration"].mean()
    overall_speedup = avg_python_time / avg_fionn_time
    col4.metric("📊 Avg Fionn Speedup", f"{overall_speedup:.2f}x", "vs Python baseline")

# Best efficiency
best_eff = filtered_df.loc[filtered_df["efficiency"].idxmax()] if len(filtered_df) > 0 else None
if best_eff is not None:
    col5.metric(
        "🎯 Best Efficiency", f"{best_eff['efficiency']:.2f}", best_eff["display_name"][:25]
    )

# --- Tabbed Analysis Views ---
tab1, tab2, tab3, tab4, tab5 = st.tabs(
    [
        "🔬 Comparative Analysis",
        "🗺️ Performance Surface",
        "📊 Faceted Breakdown",
        "🔄 Head-to-Head",
        "📋 Data Explorer",
    ]
)

with tab1:
    st.subheader("Implementation Comparison")

    col_left, col_right = st.columns(2)

    with col_left:
        # Throughput comparison
        st.markdown("#### Throughput by Implementation")
        throughput_df = filtered_df[filtered_df["bytes_per_sec"] > 0].copy()
        if len(throughput_df) > 0:
            # Determine best unit for the dataset
            max_rate = throughput_df["bytes_per_sec"].max()
            _, best_unit = format_bytes_rate(max_rate)
            divisor = {"B/s": 1, "KiB/s": 1024, "MiB/s": 1024**2, "GiB/s": 1024**3}[best_unit]
            throughput_df["throughput_scaled"] = throughput_df["bytes_per_sec"] / divisor

            fig = px.bar(
                throughput_df.sort_values("throughput_scaled", ascending=True),
                y="display_name",
                x="throughput_scaled",
                color=color_by,
                color_discrete_map=get_color_map(color_by),
                orientation="h",
                text="throughput_scaled" if show_annotations else None,
                labels={"throughput_scaled": best_unit, "display_name": "Benchmark"},
            )
            if show_annotations:
                fig.update_traces(texttemplate="%{text:.2f}", textposition="outside")
            fig.update_layout(height=400, showlegend=True, legend={"orientation": "h", "y": -0.2})
            st.plotly_chart(fig, width="stretch")

    with col_right:
        # Time comparison
        st.markdown("#### Execution Time")
        fig = px.bar(
            filtered_df.sort_values("duration", ascending=False),
            y="display_name",
            x="duration",
            color=color_by,
            color_discrete_map=get_color_map(color_by),
            orientation="h",
            text="duration" if show_annotations else None,
            labels={"duration": "Time (s)", "display_name": "Benchmark"},
            log_x=log_scale,
        )
        if show_annotations:
            fig.update_traces(texttemplate="%{text:.2f}s", textposition="outside")
        fig.update_layout(height=400, showlegend=True, legend={"orientation": "h", "y": -0.2})
        st.plotly_chart(fig, width="stretch")

    # Grouped bar chart - ALL implementations by category
    st.markdown("#### Duration by Category (All Implementations)")

    # Aggregate by category and implementation
    cat_impl_agg = (
        filtered_df.groupby(["category", "implementation"])
        .agg({"duration": "mean", "speedup": "mean"})
        .reset_index()
    )

    col_duration, col_speedup = st.columns(2)

    with col_duration:
        fig = px.bar(
            cat_impl_agg,
            x="category",
            y="duration",
            color="implementation",
            color_discrete_map=IMPL_COLORS,
            barmode="group",
            text="duration",
            labels={"duration": "Avg Duration (s)", "category": "Category"},
        )
        fig.update_traces(texttemplate="%{text:.2f}s", textposition="outside")
        fig.update_layout(height=400, xaxis_tickangle=-45, legend={"orientation": "h", "y": -0.3})
        st.plotly_chart(fig, width="stretch")

    with col_speedup:
        st.markdown("#### Speedup by Category (All Implementations)")
        fig = px.bar(
            cat_impl_agg,
            x="category",
            y="speedup",
            color="implementation",
            color_discrete_map=IMPL_COLORS,
            barmode="group",
            text="speedup",
            labels={"speedup": "Avg Speedup (x)", "category": "Category"},
        )
        fig.update_traces(texttemplate="%{text:.2f}x", textposition="outside")
        fig.update_layout(height=400, xaxis_tickangle=-45, legend={"orientation": "h", "y": -0.3})
        # Add baseline reference line at 1x
        fig.add_hline(y=1, line_dash="dash", line_color="#666", annotation_text="Baseline (1x)")
        st.plotly_chart(fig, width="stretch")

    # Distribution box plot
    st.markdown("#### Speedup Distribution by Implementation")
    fig = px.box(
        filtered_df,
        x="implementation",
        y="speedup",
        color="implementation",
        color_discrete_map=IMPL_COLORS,
        points="all",
        hover_data=["display_name", "category"],
    )
    fig.update_layout(height=350, showlegend=False)
    fig.add_hline(y=1, line_dash="dash", line_color="#666", annotation_text="Baseline")
    st.plotly_chart(fig, width="stretch")

with tab2:
    st.subheader("Performance Surface Exploration")

    col_scatter, col_heatmap = st.columns(2)

    with col_scatter:
        st.markdown("#### Time vs Memory Trade-off")
        fig = px.scatter(
            filtered_df,
            x="duration",
            y="memory_mb",
            size="speedup",
            color=color_by,
            color_discrete_map=get_color_map(color_by),
            hover_name="display_name",
            hover_data=["category", "implementation", "speedup"],
            log_x=log_scale,
            labels={"duration": "Time (s)", "memory_mb": "Memory (MB)"},
        )
        fig.update_layout(height=400)
        st.plotly_chart(fig, width="stretch")

    with col_heatmap:
        st.markdown("#### Speedup Heatmap: Category × Implementation")
        pivot = filtered_df.pivot_table(
            values="speedup", index="category", columns="implementation", aggfunc="mean"
        ).fillna(0)

        fig = px.imshow(
            pivot,
            labels={"x": "Implementation", "y": "Category", "color": "Speedup"},
            color_continuous_scale="Blues",  # Blue scale - no red
            aspect="auto",
            text_auto=".2f",
        )
        fig.update_layout(height=400)
        st.plotly_chart(fig, width="stretch")

    # 3D Performance Surface
    st.markdown("#### 3D Performance Surface")

    # Create synthetic grid for surface
    if len(filtered_df) >= 4:
        # Map implementations to colors for 3D plot
        impl_color_list = filtered_df["implementation"].map(IMPL_COLORS).tolist()

        fig = go.Figure(
            data=[
                go.Scatter3d(
                    x=filtered_df["duration"],
                    y=filtered_df["memory_mb"],
                    z=filtered_df["speedup"],
                    mode="markers+text",
                    marker={"size": 8, "color": impl_color_list, "opacity": 0.8},
                    text=filtered_df["display_name"].str[:20],
                    textposition="top center",
                    hovertemplate="<b>%{text}</b><br>Time: %{x:.2f}s<br>Memory: %{y:.1f}MB<br>Speedup: %{z:.2f}x<extra></extra>",
                )
            ]
        )
        fig.update_layout(
            scene={
                "xaxis_title": "Time (s)",
                "yaxis_title": "Memory (MB)",
                "zaxis_title": "Speedup",
            },
            height=500,
        )
        st.plotly_chart(fig, width="stretch")

with tab3:
    st.subheader("Faceted Category Breakdown")

    # Sunburst chart
    col_sun, col_tree = st.columns(2)

    with col_sun:
        st.markdown("#### Hierarchical View (by Implementation)")
        fig = px.sunburst(
            filtered_df,
            path=["implementation", "category", "operation"],
            values="speedup",
            color="implementation",
            color_discrete_map=IMPL_COLORS,
        )
        fig.update_layout(height=450)
        st.plotly_chart(fig, width="stretch")

    with col_tree:
        st.markdown("#### Treemap by Duration")
        fig = px.treemap(
            filtered_df,
            path=["category", "implementation", "display_name"],
            values="duration",
            color="implementation",
            color_discrete_map=IMPL_COLORS,
        )
        fig.update_layout(height=450)
        st.plotly_chart(fig, width="stretch")

    # Category-specific deep dives
    st.markdown("#### Per-Category Analysis")

    selected_category = st.selectbox("Select Category for Deep Dive", categories)
    cat_df = filtered_df[filtered_df["category"] == selected_category]

    if len(cat_df) > 0:
        col1, col2, col3 = st.columns(3)

        with col1:
            fig = px.bar(
                cat_df.sort_values("duration"),
                x="display_name",
                y="duration",
                color="implementation",
                color_discrete_map=IMPL_COLORS,
                text="duration",
            )
            fig.update_traces(texttemplate="%{text:.2f}s", textposition="outside")
            fig.update_layout(title="Time", xaxis_tickangle=-45, height=300)
            st.plotly_chart(fig, width="stretch")

        with col2:
            fig = px.bar(
                cat_df.sort_values("speedup", ascending=False),
                x="display_name",
                y="speedup",
                color="implementation",
                color_discrete_map=IMPL_COLORS,
                text="speedup",
            )
            fig.update_traces(texttemplate="%{text:.2f}x", textposition="outside")
            fig.update_layout(title="Speedup", xaxis_tickangle=-45, height=300)
            st.plotly_chart(fig, width="stretch")

        with col3:
            fig = px.bar(
                cat_df.sort_values("memory_mb"),
                x="display_name",
                y="memory_mb",
                color="implementation",
                color_discrete_map=IMPL_COLORS,
                text="memory_mb",
            )
            fig.update_traces(texttemplate="%{text:.1f}MB", textposition="outside")
            fig.update_layout(title="Memory", xaxis_tickangle=-45, height=300)
            st.plotly_chart(fig, width="stretch")

with tab4:
    st.subheader("Head-to-Head Comparison")

    col_select1, col_select2 = st.columns(2)

    benchmark_names = filtered_df["display_name"].tolist()

    with col_select1:
        bench1 = st.selectbox("Benchmark A", benchmark_names, index=0)
    with col_select2:
        default_idx = min(1, len(benchmark_names) - 1)
        bench2 = st.selectbox("Benchmark B", benchmark_names, index=default_idx)

    if bench1 and bench2:
        row1 = filtered_df[filtered_df["display_name"] == bench1].iloc[0]
        row2 = filtered_df[filtered_df["display_name"] == bench2].iloc[0]

        # Comparison metrics
        metrics = ["duration", "memory_mb", "speedup", "bytes_per_sec"]
        labels = ["Time (s)", "Memory (MB)", "Speedup", "Throughput (B/s)"]

        col_a, col_vs, col_b = st.columns([2, 1, 2])

        with col_a:
            st.markdown(f"### {bench1}")
            st.markdown(f"**Implementation:** {row1['implementation']}")
            st.markdown(f"**Category:** {row1['category']}")

            t_val, t_unit = format_time(row1["duration"])
            m_val, m_unit = format_memory(row1["memory_mb"])
            r_val, r_unit = format_bytes_rate(row1["bytes_per_sec"])

            st.metric("Time", f"{t_val:.2f} {t_unit}")
            st.metric("Memory", f"{m_val:.2f} {m_unit}")
            st.metric("Speedup", f"{row1['speedup']:.2f}x")
            if row1["bytes_per_sec"] > 0:
                st.metric("Throughput", f"{r_val:.2f} {r_unit}")

        with col_vs:
            st.markdown("### VS")
            # Delta calculations
            time_delta = ((row2["duration"] - row1["duration"]) / row1["duration"]) * 100
            mem_delta = ((row2["memory_mb"] - row1["memory_mb"]) / row1["memory_mb"]) * 100

            st.metric("Time Δ", f"{time_delta:+.1f}%", delta_color="inverse")
            st.metric("Memory Δ", f"{mem_delta:+.1f}%", delta_color="inverse")

            relative_speedup = row1["duration"] / row2["duration"]
            st.metric("A vs B", f"{relative_speedup:.2f}x")

        with col_b:
            st.markdown(f"### {bench2}")
            st.markdown(f"**Implementation:** {row2['implementation']}")
            st.markdown(f"**Category:** {row2['category']}")

            t_val, t_unit = format_time(row2["duration"])
            m_val, m_unit = format_memory(row2["memory_mb"])
            r_val, r_unit = format_bytes_rate(row2["bytes_per_sec"])

            st.metric("Time", f"{t_val:.2f} {t_unit}")
            st.metric("Memory", f"{m_val:.2f} {m_unit}")
            st.metric("Speedup", f"{row2['speedup']:.2f}x")
            if row2["bytes_per_sec"] > 0:
                st.metric("Throughput", f"{r_val:.2f} {r_unit}")

        # Parallel coordinates for multi-dimensional comparison
        st.markdown("#### Multi-Dimensional Comparison")
        compare_df = filtered_df[filtered_df["display_name"].isin([bench1, bench2])].copy()

        # Normalize for parallel coordinates
        for col in ["duration", "memory_mb", "speedup"]:
            col_min, col_max = filtered_df[col].min(), filtered_df[col].max()
            compare_df[f"{col}_norm"] = (compare_df[col] - col_min) / (col_max - col_min + 1e-9)

        fig = go.Figure(
            data=go.Parcoords(
                line={
                    "color": compare_df["display_name"].map({bench1: 0, bench2: 1}),
                    "colorscale": [[0, "#636EFA"], [1, "#00CC96"]],
                },
                dimensions=[
                    {"label": "Time (norm)", "values": compare_df["duration_norm"]},
                    {"label": "Memory (norm)", "values": compare_df["memory_mb_norm"]},
                    {"label": "Speedup (norm)", "values": compare_df["speedup_norm"]},
                ],
            )
        )
        fig.update_layout(height=300)
        st.plotly_chart(fig, width="stretch")

with tab5:
    st.subheader("Data Explorer")

    # Column selector
    all_cols = filtered_df.columns.tolist()
    display_cols = st.multiselect(
        "Select Columns",
        all_cols,
        default=[
            "display_name",
            "implementation",
            "category",
            "duration",
            "memory_mb",
            "speedup",
            "throughput_val",
            "throughput_unit",
        ],
    )

    # Sorting
    sort_col = st.selectbox(
        "Sort By",
        display_cols,
        index=display_cols.index("speedup") if "speedup" in display_cols else 0,
    )
    sort_order = st.radio("Order", ["Descending", "Ascending"], horizontal=True)

    display_df = filtered_df[display_cols].sort_values(
        sort_col, ascending=(sort_order == "Ascending")
    )

    # Format numeric columns
    format_dict = {}
    for col in display_cols:
        if col in ["duration", "speedup", "efficiency", "throughput_val", "mem_val"] or col in [
            "memory_mb",
            "bytes_per_sec",
            "file_size_bytes",
        ]:
            format_dict[col] = "{:.2f}"

    st.dataframe(display_df.style.format(format_dict), width="stretch", height=400)

    # Export options
    col_export1, col_export2 = st.columns(2)
    with col_export1:
        csv = display_df.to_csv(index=False)
        st.download_button("📥 Download CSV", csv, "benchmark_data.csv", "text/csv")
    with col_export2:
        json_str = display_df.to_json(orient="records", indent=2)
        st.download_button("📥 Download JSON", json_str, "benchmark_data.json", "application/json")

# --- Insights Section ---
st.divider()
st.header("💡 Key Insights")

insights_col1, insights_col2 = st.columns(2)

with insights_col1:
    st.markdown("#### Performance Observations")

    # Auto-generate insights
    if len(fionn_df) > 0 and len(python_df) > 0:
        # Fastest Fionn benchmark
        fastest_fionn = fionn_df.loc[fionn_df["duration"].idxmin()]
        st.success(
            f"**Fastest Fionn:** {fastest_fionn['display_name']} ({fastest_fionn['duration']:.2f}s)"
        )

        # Highest speedup
        max_speedup_row = filtered_df.loc[filtered_df["speedup"].idxmax()]
        st.success(
            f"**Highest Speedup:** {max_speedup_row['display_name']} ({max_speedup_row['speedup']:.2f}x)"
        )

        # Memory efficiency leader
        fionn_mem_avg = fionn_df["memory_mb"].mean()
        python_mem_avg = python_df["memory_mb"].mean()
        mem_savings = ((python_mem_avg - fionn_mem_avg) / python_mem_avg) * 100
        if mem_savings > 0:
            st.info(f"**Fionn Memory Savings:** {mem_savings:.1f}% avg vs Python")
        else:
            st.info(f"**Memory Overhead:** Fionn uses {-mem_savings:.1f}% more memory on average")

with insights_col2:
    st.markdown("#### Recommendations")

    # Category-specific recommendations
    for cat in categories[:3]:
        cat_df = filtered_df[filtered_df["category"] == cat]
        if len(cat_df) > 0:
            best = cat_df.loc[cat_df["speedup"].idxmax()]
            st.markdown(f"**{cat}:** Use `{best['display_name']}` ({best['speedup']:.2f}x speedup)")

# --- Footer ---
st.divider()
st.caption(
    "Fionn Performance Explorer | Built with Streamlit + Plotly | Data from DLT benchmark suite"
)
