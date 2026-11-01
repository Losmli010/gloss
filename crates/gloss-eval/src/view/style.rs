//! 检视页样式：单文件（HTML）的内联层叠样式（CSS）——系统字体、白底、
//! 灰边卡片、浅灰块、内容宽 1100 像素居中，窄屏保留标识、输入、契约与
//! 回归四列。徽标四色与曲线共用色经占位符注入（单一色源住趋势模块）。

pub(crate) fn stylesheet() -> String {
    TEMPLATE
        .replace("@text", crate::trend::COLOR_TEXT)
        .replace("@muted", crate::trend::COLOR_MUTED)
        .replace("@green", crate::trend::COLOR_GREEN)
        .replace("@amber", crate::trend::COLOR_AMBER)
        .replace("@red", crate::trend::COLOR_RED)
        .replace("@gray", crate::trend::COLOR_GRAY)
}

const TEMPLATE: &str = r##"body{font-family:-apple-system,'Segoe UI','PingFang SC','Hiragino Sans GB','Microsoft YaHei',sans-serif;color:@text;max-width:1100px;margin:24px auto;padding:0 16px;background:#fff}
h1{font-size:20px;margin:0 0 16px}h2{font-size:14px;margin:26px 0 8px}
.secthead{display:flex;justify-content:space-between;align-items:baseline}
.export{font:inherit;font-size:13px;font-weight:600;background:@text;color:#fff;border:1px solid @text;border-radius:14px;padding:4px 14px;cursor:pointer}
.export .count{background:#fff;color:@text;border-radius:9px;padding:0 7px;margin-left:4px;font-size:12px}
.legend-row{display:flex;gap:16px;flex-wrap:wrap;font-size:11px;color:@muted;margin-bottom:8px}
.lgroup{display:inline-flex;align-items:center;gap:4px}
.lgroup b{font-weight:600}
.sw{display:inline-block;width:12px;height:12px;border-radius:2px;background:#eceff2}
.sw.g{background:@green}.sw.a{background:@amber}.sw.r{background:@red}.sw.gr{background:#9aa2ab}
.dash{border:1px solid #d7dce3;border-radius:8px;padding:12px 14px;margin-top:4px}
.vchart{display:flex;gap:16px;align-items:flex-end;flex-wrap:wrap;margin-bottom:8px}
.vgroup{display:flex;flex-direction:column;align-items:center;gap:4px}
.vgroup.sum{border-left:1px dashed #d7dce3;padding-left:16px}
.vbars{display:flex;gap:6px;align-items:flex-end;height:100px;background:linear-gradient(to top,#eceff2 1px,transparent 1px) 0 0/100% 25%}
.vcol{display:flex;flex-direction:column;align-items:center;gap:3px}
.vbar{width:22px;border-radius:2px 2px 0 0;background:@green}
.vbar.gray{background:@gray}
.vnum{font-size:11px;color:@text;font-weight:600}
.vsub{font-size:10px;color:@gray}
.vlabel{font-size:12px;font-weight:600;color:@text}
.vnote{font-size:11px;color:@gray;margin:6px 0 20px}
.tchart{--tcols:56px 40px 72px 72px 72px 56px 48px 48px 48px 48px 56px 56px 40px 40px 44px;font-size:12px;margin-top:18px}
.trow{display:grid;grid-template-columns:var(--tcols);gap:10px;align-items:center;padding:7px 0;border-bottom:1px solid #eceff2}
.trow.head{padding:2px 0 6px;border-bottom:1px solid #d7dce3}
.trow.sum{border-top:1px dashed #d7dce3;border-bottom:none}
.thead{font-size:11px;color:@muted}
.tname{color:@text;font-weight:600}
.tnum{text-align:right;color:@text}
.filters{margin:8px 0}
.filter-row{display:flex;flex-wrap:wrap;align-items:center;margin:6px 0}
.filter-row .rname{font-size:12px;color:@muted;width:52px;flex:none}
.filters button,.filter-row button{font:inherit;font-size:12px;border:1px solid #d7dce3;background:#fff;border-radius:14px;padding:3px 12px;margin-right:6px;cursor:pointer;color:@muted}
.filters button.on,.filter-row button.on{background:@text;color:#fff;border-color:@text}
.cases-head{display:grid;grid-template-columns:110px 1fr 44px 40px 64px 64px 52px;gap:8px;padding:5px 10px;font-size:11px;color:@muted;border-bottom:1px solid #d7dce3;margin-top:6px}
.empty{color:@muted;font-size:13px;padding:14px 0}
details.case{border:1px solid #d7dce3;border-radius:6px;margin:8px 0}
details.case>summary{display:grid;grid-template-columns:110px 1fr 44px 40px 64px 64px 52px;gap:8px;align-items:center;padding:7px 10px;cursor:pointer;font-size:13px;list-style:none}
details.case>summary::-webkit-details-marker{display:none}
details.case[open]{background:#fcfcfd}
.badge{font-size:11px;font-weight:600;border-radius:9px;padding:1px 8px;text-align:center;white-space:nowrap}
.badge.green{background:#e3f4e9;color:@green}.badge.red{background:#fbe7e7;color:@red}
.badge.amber{background:#f9efd8;color:@amber}.badge.gray{background:#eceff2;color:@gray}
.cid{color:@muted;font-size:12px}.ctext{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.ckind,.crounds{color:@muted;font-size:12px;white-space:nowrap}
.coverall{font-size:12px;font-weight:650}
.case-body{padding:6px 12px 12px;border-top:1px dashed #d7dce3}
.cols{display:grid;grid-template-columns:1fr 1.4fr 1.4fr;gap:10px;margin:8px 0}
.cols.two{grid-template-columns:1fr 1.6fr}
.colhead{font-size:11px;color:@muted;margin-bottom:3px}
.colhead .hl{color:@red;font-weight:600}
pre{margin:0;background:#f6f8fa;border:1px solid #d7dce3;border-radius:6px;padding:8px;font-size:12px;font-family:ui-monospace,Menlo,monospace;white-space:pre-wrap;word-break:break-all;max-height:200px;overflow:auto}
.reference{margin:8px 0}
.settle{margin:10px 0;padding:6px 0;border-top:1px dashed #d7dce3;border-bottom:1px dashed #d7dce3}
.srow{display:flex;gap:8px;align-items:baseline;font-size:12px;margin:4px 0}
.sname{flex:none;width:58px;color:@muted;font-weight:600}
.sval{color:@text}
.sval .bad{color:@red;font-weight:600}
.sval .warn{color:@amber;font-weight:600}
.sval .ok{color:@green}
.markline{display:flex;gap:8px;align-items:center;margin:10px 0;padding:8px 0}
.markbtn{font:inherit;font-size:12px;border:1px solid #d7dce3;background:#fff;border-radius:14px;padding:3px 14px;cursor:pointer;color:@muted}
.markbtn.on.pass{background:@green;border-color:@green;color:#fff}
.markbtn.on.fail{background:@red;border-color:@red;color:#fff}
.markline input{font:inherit;font-size:12px;border:1px solid #d7dce3;border-radius:6px;padding:4px 8px;flex:1;max-width:320px}
.markline .hint{font-size:11px;color:@gray}
details.rounds{margin:6px 0}
details.rounds summary{font-size:12px;color:@muted;cursor:pointer}
.round{margin:8px 0}
.round-head{font-size:12px;color:@muted;margin-bottom:2px}
.tag{background:#fbe7e7;color:@red;border-radius:8px;padding:0 8px;font-size:11px}
@media (max-width:860px){details.case>summary{grid-template-columns:110px 1fr 64px 64px}.cases-head{grid-template-columns:110px 1fr 64px 64px}.ckind,.crounds,.coverall,.hd-kind,.hd-rounds,.hd-overall{display:none}.cols{grid-template-columns:1fr}}
"##;
