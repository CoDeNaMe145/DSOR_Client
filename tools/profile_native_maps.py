"""Windows native fixed-view profiling; see docs/native-map-performance.md.

Only launches/terminates its own offline client. Existing UI replay prevents
background frame throttling without changing the desktop foreground window.
Outputs are local diagnostic artifacts, never a claim of a clean benchmark.
"""
import argparse, ctypes, hashlib, json, os, pathlib, re, subprocess, time

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / '.local/logs/map-fps'
OUT.mkdir(parents=True, exist_ok=True)
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('map', choices=['kingscity', 'swerdfield'])
p.add_argument('name')
p.add_argument('--flags', default='')
p.add_argument('--seconds', type=int, default=35)
p.add_argument('--exe', default='target/release/dsor-client.exe')
p.add_argument('--map-only', action='store_true')
p.add_argument('--audit', action='store_true')
p.add_argument('--capture', action='store_true')
p.add_argument('--position', nargs=3, type=float, metavar=('X','Y','Z'))
p.add_argument('--route', type=pathlib.Path, help='offline position/capture replay, for cell unload/reload checks')
p.add_argument('--ui-route', type=pathlib.Path, help='UI actions/captures to merge with the focus replay')
p.add_argument('--check-skins', action='store_true', help='expensive live joint-ownership checks; never use for FPS timing')
p.add_argument('--env', action='append', default=[], metavar='KEY=VALUE',
               help='extra client environment, e.g. DSOR_SYSTEM_PROFILE=dsor_client::ui (repeatable)')
a = p.parse_args()
if os.name != 'nt': p.error('this runner requires Windows')
map_name, pos = {'kingscity': ('a0200_kingscity', [155, 1, -54]),
                 'swerdfield': ('a0007_pastures_pve', [114, 0, 82])}[a.map]
if a.position is not None: pos = a.position
if a.seconds < 20: p.error('--seconds must be at least 20 to allow loading and warm-up')
tag = f'{a.map}-{a.name}'
route = OUT / f'{tag}.replay.json'
steps = [dict(frame=1, position=pos)]
if a.capture or a.audit: steps.append(dict(frame=1200, capture=str(OUT/f'{tag}.png')))
route.write_text(a.route.read_text() if a.route else json.dumps(steps))
env = {k:v for k,v in os.environ.items() if not k.startswith('DSOR_')}
env.update(DSOR_FLAGS=','.join(filter(None,['frameprofile', 'renderaudit' if a.audit else '', a.flags])),
           DSOR_OCCLUSION_REPLAY=str(route), DSOR_ASSETS=str(ROOT/'assets'),
           RUST_LOG='info,wgpu=warn,naga=warn', NO_COLOR='1')
# The existing UI replay marks the test window focused without stealing desktop
# focus. Keep the cursor outside the viewport and suppress winit's background
# 60-Hz throttle; otherwise disabling a subsystem just increases sleep time.
focus_route = OUT / f'{tag}.focus.json'
focus_steps = [dict(frame=f,point=[-1000,-1000]) for f in range(1,20001,10)]
if a.ui_route: focus_steps.extend(json.loads(a.ui_route.read_text()))
focus_route.write_text(json.dumps(sorted(focus_steps, key=lambda step: step['frame'])))
env['DSOR_UI_REPLAY'] = str(focus_route)
if a.map_only: env['DSOR_NO_DECO'] = '1'
if a.audit: env['DSOR_RENDER_AUDIT'] = str(OUT/f'{tag}.audit.json')
if a.check_skins: env['DSOR_CHECK_SKIN'] = '1'
for extra in a.env:
    key, _, value = extra.partition('=')
    env[key] = value
cmd = [str(ROOT/a.exe), map_name, '--offline', '--play', '1', '0']
if not a.map_only: cmd.append('--npcs')
with (ROOT/a.exe).open('rb') as binary:
    digest = hashlib.sha256()
    for chunk in iter(lambda: binary.read(1024 * 1024), b''): digest.update(chunk)
    exe_sha256 = digest.hexdigest()
log_path = OUT / f'{tag}.log'
user32 = ctypes.windll.user32
user32.GetForegroundWindow.restype = ctypes.c_void_p
user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
focus = []
def compilers():
    result = subprocess.run(['powershell','-NoProfile','-Command',
        'Get-Process rustc,link -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,CPU | ConvertTo-Json -Compress'],
        capture_output=True,text=True,creationflags=subprocess.CREATE_NO_WINDOW,timeout=10)
    return result.stdout.strip() or result.stderr.strip() or 'none'
compiler_before = compilers()
with log_path.open('w') as log:
    proc = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=log, stderr=log,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        start = time.monotonic()
        while proc.poll() is None and time.monotonic()-start < a.seconds:
            owner = ctypes.c_ulong()
            user32.GetWindowThreadProcessId(user32.GetForegroundWindow(), ctypes.byref(owner))
            focus.append([time.monotonic()-start, owner.value == proc.pid])
            time.sleep(.25)
    finally:
        timed_out = proc.poll() is None
        if timed_out: proc.terminate(); proc.wait(timeout=10)
compiler_after = compilers()
data = re.sub(r'\x1b\[[0-9;]*m', '', log_path.read_text(errors='replace'))
report = dict(tag=tag, map=map_name, position=pos, flags=env['DSOR_FLAGS'],
              exe=a.exe, seconds=a.seconds, exit_code=proc.returncode, timed_out=timed_out,
              exe_sha256=exe_sha256,
              route=str(a.route) if a.route else None,
              replay_focus=True,
              compiler_before=compiler_before, compiler_after=compiler_after,
              focus=focus,
              windows=re.findall(r'frame profile: \d+ frames, .*', data),
              hud=re.findall(r'\d+ FPS \([^\n]+', data),
              stages=re.findall(r'frame profile: (\w+::\w+) took ([\d.]+) ms', data),
              ui_work=re.findall(r'ui work: .*', data),
              errors=[x for x in data.splitlines() if 'ERROR' in x or 'panicked' in x])
(OUT/f'{tag}.summary.json').write_text(json.dumps(report,indent=2))
print(json.dumps({k:v for k,v in report.items() if k not in ('stages','focus')}, indent=2),flush=True)
# The fixed view is idle: after loading, the interface must rebuild nothing
# (docs/ui.md, "Per-frame cost"). A UI route legitimately opens windows.
idle = re.findall(r'ui work: (\d+) layouts prepared, (\d+) system views, (\d+) quest lists(?:, (\d+) status bars)?(?:, (\d+) previews)?(?:, (\d+) game data bars)?', data)
if not a.ui_route and len(idle) >= 2 and any(int(n or 0) for n in idle[-1]):
    print('WARNING: the interface rebuilt something every window while idle: ' + report['ui_work'][-1], flush=True)
