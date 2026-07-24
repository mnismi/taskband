'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const m = require('./claude-usage.js');

const OPEN = '\uFF3B';
const CLOSE = '\uFF3D';
const FILLED = '\uFFED';
const EMPTY = '\uFF65';
const MIN = 60_000;

/** Expected bar with `n` of 10 segments filled. */
function bar(n) {
    return OPEN + FILLED.repeat(n) + EMPTY.repeat(10 - n) + CLOSE;
}

test('progressBar fills one segment per ten percent', () => {
    assert.equal(m.progressBar(0), bar(0));
    assert.equal(m.progressBar(20), bar(2));
    assert.equal(m.progressBar(50), bar(5));
    assert.equal(m.progressBar(61), bar(6));
    assert.equal(m.progressBar(100), bar(10));
});

test('progressBar clamps out-of-range percentages', () => {
    assert.equal(m.progressBar(150), bar(10));
    assert.equal(m.progressBar(-10), bar(0));
});

test('humanizeUntil formats minutes, hours and days', () => {
    assert.equal(m.humanizeUntil(0, 45 * MIN), '45m');
    assert.equal(m.humanizeUntil(0, 150 * MIN), '2h 30m');
    assert.equal(m.humanizeUntil(0, 1560 * MIN), '1d 2h');
});

test('humanizeUntil reports a reset that has passed as now', () => {
    assert.equal(m.humanizeUntil(0, 0), 'now');
    assert.equal(m.humanizeUntil(0, -5 * MIN), 'now');
});

test('formatLines reproduces the two-line taskbar display', () => {
    const usage = {
        sessionPct: 20,
        sessionResetsAt: 226 * MIN, // 3h 46m
        weeklyPct: 61,
        weeklyResetsAt: 3000 * MIN, // 2d 2h
    };
    assert.deepEqual(m.formatLines(usage, 0), [
        `5H ${bar(2)}  20% · 3h 46m`,
        `7D ${bar(6)}  61% · 2d 2h`,
    ]);
});

test('line right-aligns the percentage to three characters', () => {
    assert.equal(m.line('5H', 7, ''), `5H ${bar(1)}   7%`);
    assert.equal(m.line('7D', 100, ''), `7D ${bar(10)} 100%`);
});

const SAMPLE = JSON.stringify({
    five_hour: { utilization: 73.0, resets_at: '2026-07-22T13:49:59.949876+00:00' },
    seven_day: { utilization: 28.0, resets_at: '2026-07-26T14:59:59.949896+00:00' },
    seven_day_opus: null,
    seven_day_sonnet: null,
});

const CONFIG = { orgId: 'org', cookie: 'c', userAgent: 'ua' };

test('parseUsage reads both windows from a real response shape', () => {
    const u = m.parseUsage(SAMPLE);
    assert.equal(u.sessionPct, 73);
    assert.equal(u.weeklyPct, 28);
    assert.equal(u.sessionResetsAt, Date.parse('2026-07-22T13:49:59.949876+00:00'));
    assert.equal(u.weeklyResetsAt, Date.parse('2026-07-26T14:59:59.949896+00:00'));
});

test('parseUsage rounds fractional utilization', () => {
    const body = JSON.stringify({
        five_hour: { utilization: 12.6, resets_at: '2026-07-22T13:49:59+00:00' },
        seven_day: { utilization: 4.4, resets_at: '2026-07-26T14:59:59+00:00' },
    });
    const u = m.parseUsage(body);
    assert.equal(u.sessionPct, 13);
    assert.equal(u.weeklyPct, 4);
});

test('parseUsage names the window that is missing', () => {
    const noFive = JSON.stringify({
        seven_day: { utilization: 1, resets_at: '2026-07-26T14:59:59+00:00' },
    });
    assert.throws(() => m.parseUsage(noFive), /five_hour/);

    const noSeven = JSON.stringify({
        five_hour: { utilization: 1, resets_at: '2026-07-22T13:49:59+00:00' },
    });
    assert.throws(() => m.parseUsage(noSeven), /seven_day/);
});

test('parseUsage rejects a body that is not JSON', () => {
    assert.throws(() => m.parseUsage('<html>nope</html>'), /bad usage JSON/);
});

test('fetchUsage requests the organization usage endpoint', async () => {
    let seen = null;
    await m.fetchUsage(CONFIG, async (url) => {
        seen = url;
        return { status: 200, body: SAMPLE };
    });
    assert.equal(seen, 'https://claude.ai/api/organizations/org/usage');
});

test('fetchUsage maps 200 to a parsed usage', async () => {
    const out = await m.fetchUsage(CONFIG, async () => ({ status: 200, body: SAMPLE }));
    assert.equal(out.kind, 'ok');
    assert.equal(out.usage.sessionPct, 73);
});

test('fetchUsage maps 401 and 403 to auth', async () => {
    for (const status of [401, 403]) {
        const out = await m.fetchUsage(CONFIG, async () => ({ status, body: '' }));
        assert.equal(out.kind, 'auth', `status ${status}`);
    }
});

test('fetchUsage maps other non-2xx to http with the status', async () => {
    const out = await m.fetchUsage(CONFIG, async () => ({ status: 500, body: '' }));
    assert.equal(out.kind, 'http');
    assert.equal(out.status, 500);
});

test('fetchUsage maps a transport failure to offline', async () => {
    const out = await m.fetchUsage(CONFIG, async () => {
        throw new Error('boom');
    });
    assert.equal(out.kind, 'offline');
    assert.match(out.detail, /boom/);
});

test('fetchUsage maps an unparseable 200 body to bad-response', async () => {
    const out = await m.fetchUsage(CONFIG, async () => ({ status: 200, body: 'not json' }));
    assert.equal(out.kind, 'bad-response');
});

test('render prints two bar lines for a good outcome', () => {
    const usage = {
        sessionPct: 20,
        sessionResetsAt: 226 * MIN,
        weeklyPct: 61,
        weeklyResetsAt: 3000 * MIN,
    };
    const text = m.render({ kind: 'ok', usage }, 0);
    assert.equal(text.split('\n').length, 2);
    assert.ok(text.startsWith('5H '));
});

test('render prints one short line for every failure', () => {
    assert.equal(m.render({ kind: 'no-config' }, 0), 'Claude: no config');
    assert.equal(m.render({ kind: 'auth' }, 0), 'Claude: auth expired');
    assert.equal(m.render({ kind: 'http', status: 500 }, 0), 'Claude: HTTP 500');
    assert.equal(m.render({ kind: 'offline' }, 0), 'Claude: offline');
    assert.equal(m.render({ kind: 'bad-response' }, 0), 'Claude: bad response');
});

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

/** A throwaway directory holding a config.json with the given contents. */
function tempConfigDir(contents) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-usage-test-'));
    if (contents !== null) {
        fs.writeFileSync(path.join(dir, 'config.json'), contents, 'utf8');
    }
    return dir;
}

test('loadConfig reads the three required keys', () => {
    const dir = tempConfigDir(
        JSON.stringify({ orgId: 'o', cookie: 'c', userAgent: 'ua', extra: 1 })
    );
    const cfg = m.loadConfig(dir);
    assert.equal(cfg.orgId, 'o');
    assert.equal(cfg.cookie, 'c');
    assert.equal(cfg.userAgent, 'ua');
});

test('loadConfig explains a missing file', () => {
    const dir = tempConfigDir(null);
    assert.throws(() => m.loadConfig(dir), /cannot read/);
});

test('loadConfig explains malformed JSON', () => {
    const dir = tempConfigDir('{ nope');
    assert.throws(() => m.loadConfig(dir), /bad JSON/);
});

test('loadConfig names the key that is missing or empty', () => {
    const missing = tempConfigDir(JSON.stringify({ cookie: 'c', userAgent: 'ua' }));
    assert.throws(() => m.loadConfig(missing), /orgId/);

    const empty = tempConfigDir(JSON.stringify({ orgId: '', cookie: 'c', userAgent: 'ua' }));
    assert.throws(() => m.loadConfig(empty), /orgId/);
});
