"""Prints the result tables from one or more JSONL result files."""
import json, sys

def med(x): return x["median"] if isinstance(x, dict) else x

for path in sys.argv[1:]:
    print(f"== {path}")
    for line in open(path, encoding="utf-8"):
        d = json.loads(line)
        s = d["section"]
        if s == "platform":
            print("platform", d)
        elif s == "synthetic":
            r = d["result"]
            if "error" in r: print("synthetic", r); continue
            print(f"synthetic {r['mode']:26} search {med(r['search_ms']):8.3f}  aes {med(r['decrypt_mib_ms']):8.3f}  compile {r.get('compile_ms','-')}  inst {r.get('instantiate_ms','-')}  linmem {r.get('linear_memory_after_instantiate','-')}/{r.get('linear_memory_after_workload','-')}")
        elif s == "signing":
            print(f"signing verify {med(d['verify_ms'])} ms, wasmsign2 verify {med(d['wasmsign2']['verify_ms'])} ms; code:{d['code_byte_flipped']} manifest:{d['manifest_host_changed']} appended:{d['section_appended_after_signature']} takeover:{d['other_key_same_id']} ws2 tampered:{d['wasmsign2']['tampered']} ws2 other:{d['wasmsign2']['other_key']}")
        elif s == "soundcloud":
            if "error" in d: print("soundcloud", d["mode"], d["error"]); continue
            c = d["cold_search"]; rp = d["replay"]; n = d["native"]
            print(f"soundcloud {d['mode']:26} compile {d['compile_ms']:7.1f} inst {d['instantiate_ms']:.3f} | cold wall {c['wall_ms']:.0f} net {c['network_ms']:.0f} plugin {c['plugin_ms']:.1f} | search wall {med(d['search']['wall_ms']):.0f} net {med(d['search']['network_ms']):.0f} plugin {med(d['search']['plugin_ms']):.2f} | resolve wall {med(d['resolve']['wall_ms']):.0f} plugin {med(d['resolve']['plugin_ms']):.3f} | replay cold {rp['cold_search_incl_client_id_scan_ms']:.1f} search {med(rp['search_plugin_ms']):.2f} resolve {med(rp['resolve_plugin_ms']):.3f} | native cold {n['cold_search_incl_client_id_scan_ms']:.2f} search {med(n['search_ms']):.2f} resolve {med(n['resolve_ms']):.3f} | {d['results']} stream_ok={d['stream']['playlist_ok']}")
        elif s == "ten_plugins":
            if "error" in d: print("ten", d["mode"], d["error"]); continue
            p = d["process_memory_per_plugin"]
            print(f"ten {d['mode']:26} verify {med(d['verify_ms']):.3f} compile {med(d['compile_ms']):.1f} inst {med(d['instantiate_ms']):.3f} | linmem {d['linear_memory_per_instance']} | code {p['compiled_code']} inst {p['instance']} call {p['after_search']} | total {d['process_memory_total']} | cache {d['compiled_cache']}")
