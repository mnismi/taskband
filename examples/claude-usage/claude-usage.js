'use strict';

// Taskband module: Claude usage as two progress-bar lines.
// See README.md for setup. Run the tests with: node --test

const fs = require('node:fs');
const path = require('node:path');

// Glyphs are written as escapes rather than literals so that re-encoding this
// file cannot corrupt them:
//   ［ [  ］ ]  fullwidth brackets
//   ￭    filled segment
//   ･    empty segment
//   ·    middle dot separator
const SEGMENTS = 10;
const FILLED = '\uFFED';
const EMPTY = '\uFF65';
const OPEN = '\uFF3B';
const CLOSE = '\uFF3D';
const DOT = '\u00B7';

/** A 10-segment bar, one segment per 10%. Out-of-range input is clamped. */
function progressBar(pct) {
    const clamped = Math.min(Math.max(pct, 0), 100);
    const filled = Math.round((clamped / 100) * SEGMENTS);
    return OPEN + FILLED.repeat(filled) + EMPTY.repeat(SEGMENTS - filled) + CLOSE;
}

/** Coarse countdown to `targetMs`: "1d 2h", "2h 30m", "45m", or "now". */
function humanizeUntil(nowMs, targetMs) {
    const ms = targetMs - nowMs;
    if (ms <= 0) {
        return 'now';
    }
    const totalMinutes = Math.floor(ms / 60_000);
    const days = Math.floor(totalMinutes / 1440);
    const hours = Math.floor((totalMinutes % 1440) / 60);
    const minutes = totalMinutes % 60;
    if (days > 0) {
        return `${days}d ${hours}h`;
    }
    if (hours > 0) {
        return `${hours}h ${minutes}m`;
    }
    return `${minutes}m`;
}

function line(tag, pct, suffix) {
    return `${tag} ${progressBar(pct)} ${String(pct).padStart(3)}%${suffix}`;
}

/** The two bar lines, session over weekly. */
function formatLines(usage, nowMs) {
    return [
        line('5H', usage.sessionPct, ` ${DOT} ${humanizeUntil(nowMs, usage.sessionResetsAt)}`),
        line('7D', usage.weeklyPct, ` ${DOT} ${humanizeUntil(nowMs, usage.weeklyResetsAt)}`),
    ];
}

// Taskband's worker (src/plugin.rs) runs modules sequentially on one thread,
// so this timeout is also the longest this module can stall the whole bar.
const API_TIMEOUT_MS = 5_000;

/** Pull `utilization` and `resets_at` out of one window, or say what is missing. */
function readWindow(win, name) {
    const util = win && typeof win.utilization === 'number' ? win.utilization : null;
    const resets = win && typeof win.resets_at === 'string' ? win.resets_at : null;
    if (util === null || resets === null) {
        throw new Error(`Response missing ${name} utilization/resets_at`);
    }
    const at = Date.parse(resets);
    if (Number.isNaN(at)) {
        throw new Error(`bad timestamp ${resets}`);
    }
    return { pct: Math.round(util), at };
}

function parseUsage(body) {
    let raw;
    try {
        raw = JSON.parse(body);
    } catch (err) {
        throw new Error(`bad usage JSON: ${err.message}`);
    }
    const five = readWindow(raw.five_hour, 'five_hour');
    const seven = readWindow(raw.seven_day, 'seven_day');
    return {
        sessionPct: five.pct,
        sessionResetsAt: five.at,
        weeklyPct: seven.pct,
        weeklyResetsAt: seven.at,
    };
}

/**
 * Fetch usage through an injected `send`, so tests drive every branch with no
 * network. Never throws: every failure becomes an outcome.
 */
async function fetchUsage(config, send) {
    const url = `https://claude.ai/api/organizations/${config.orgId}/usage`;
    let resp;
    try {
        resp = await send(url);
    } catch (err) {
        return { kind: 'offline', detail: err.message };
    }
    if (resp.status === 401 || resp.status === 403) {
        return { kind: 'auth' };
    }
    if (resp.status < 200 || resp.status >= 300) {
        return { kind: 'http', status: resp.status };
    }
    try {
        return { kind: 'ok', usage: parseUsage(resp.body) };
    } catch (err) {
        return { kind: 'bad-response', detail: err.message };
    }
}

/** Exactly what goes to stdout. Failures are one short line, never blank. */
function render(outcome, nowMs) {
    switch (outcome.kind) {
        case 'ok':
            return formatLines(outcome.usage, nowMs).join('\n');
        case 'auth':
            return 'Claude: auth expired';
        case 'http':
            return `Claude: HTTP ${outcome.status}`;
        case 'offline':
            return 'Claude: offline';
        case 'bad-response':
            return 'Claude: bad response';
        case 'no-config':
            return 'Claude: no config';
        default:
            return 'Claude: error';
    }
}

const REQUIRED_KEYS = ['orgId', 'cookie', 'userAgent'];

/** Read and validate `config.json` from `dir`. */
function loadConfig(dir) {
    const file = path.join(dir, 'config.json');
    let text;
    try {
        text = fs.readFileSync(file, 'utf8');
    } catch (err) {
        throw new Error(`cannot read ${file}: ${err.message}`);
    }
    let cfg;
    try {
        cfg = JSON.parse(text);
    } catch (err) {
        throw new Error(`bad JSON in ${file}: ${err.message}`);
    }
    for (const key of REQUIRED_KEYS) {
        if (typeof cfg[key] !== 'string' || cfg[key].length === 0) {
            throw new Error(`${file}: "${key}" is missing or empty`);
        }
    }
    return { orgId: cfg.orgId, cookie: cfg.cookie, userAgent: cfg.userAgent };
}

/** The real network sender. Headers mirror what claude.ai's own web client sends. */
function realSender(config) {
    return async (url) => {
        const resp = await fetch(url, {
            headers: {
                accept: '*/*',
                'content-type': 'application/json',
                'anthropic-client-platform': 'web_claude_ai',
                referer: 'https://claude.ai/new',
                'user-agent': config.userAgent,
                cookie: config.cookie,
            },
            signal: AbortSignal.timeout(API_TIMEOUT_MS),
        });
        return { status: resp.status, body: await resp.text() };
    };
}

async function main() {
    let config;
    try {
        config = loadConfig(__dirname);
    } catch (err) {
        console.error(`claude-usage: ${err.message}`);
        console.log(render({ kind: 'no-config' }, Date.now()));
        return;
    }
    const outcome = await fetchUsage(config, realSender(config));
    if (outcome.kind !== 'ok') {
        console.error(`claude-usage: ${outcome.kind}${outcome.detail ? `: ${outcome.detail}` : ''}`);
    }
    console.log(render(outcome, Date.now()));
}

if (require.main === module) {
    main();
}

module.exports = {
    progressBar,
    humanizeUntil,
    line,
    formatLines,
    parseUsage,
    fetchUsage,
    render,
    loadConfig,
    realSender,
    API_TIMEOUT_MS,
};
