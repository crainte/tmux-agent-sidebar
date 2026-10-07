-- Run: luajit .lain/tmux_agent_sidebar_spec.lua
-- Stubs the `lain` global so the bridge can be exercised without lain.
local here = arg[0]:match("(.*/)") or "./"
package.path = here .. "?.lua;" .. package.path

local function encode(v)
  local t = type(v)
  if t == "string" then
    return '"' .. v:gsub('[%c"\\]', function(c)
      return string.format("\\u%04x", c:byte())
    end) .. '"'
  elseif t == "table" then
    local keys = {}
    for k in pairs(v) do
      keys[#keys + 1] = k
    end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts + 1] = encode(k) .. ":" .. encode(v[k])
    end
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return tostring(v)
end

local function setup(env)
  local s = { autocmds = {}, jobs = {}, current = "ses-1", live = {} }
  env = env or { TMUX_PANE = "%1" }
  _G.lain = {
    uv = {
      os_getenv = function(k)
        return env[k]
      end,
      os_homedir = function()
        return "/home/u"
      end,
      cwd = function()
        return "/work"
      end,
    },
    fn = {
      executable = function(p)
        return p == "/home/u/code/tmux-agent-sidebar/hook.sh" and 1 or 0
      end,
      jobstart = function(cmd, opts)
        s.jobs[#s.jobs + 1] = { cmd = cmd, opts = opts }
        return #s.jobs
      end,
    },
    json = { encode = encode },
    session = {
      current = function()
        return s.current
      end,
    },
    api = {
      create_autocmd = function(event, opts)
        s.autocmds[event] = opts.callback
      end,
    },
  }
  package.loaded.tmux_agent_sidebar = nil
  require("tmux_agent_sidebar")
  s.fire = function(event, data)
    s.autocmds[event]({ event = event, data = data })
  end
  s.last = function()
    return s.jobs[#s.jobs]
  end
  return s
end

local failures, count = 0, 0
local function test(name, fn)
  count = count + 1
  local ok, err = pcall(fn)
  if not ok then
    failures = failures + 1
    print("FAIL " .. name .. "\n  " .. tostring(err))
  end
end

local function eq(a, b)
  if a ~= b then
    error(string.format("expected %q, got %q", tostring(b), tostring(a)), 2)
  end
end

local function has(s, needle)
  if not s:find(needle, 1, true) then
    error(string.format("%q not found in %q", needle, s), 2)
  end
end

local HOOK = "bash '/home/u/code/tmux-agent-sidebar/hook.sh' lain "

test("no-op outside tmux", function()
  local s = setup({})
  eq(#s.jobs, 0)
  eq(next(s.autocmds), nil)
end)

test("session-start on load with plugin owner", function()
  local s = setup()
  eq(#s.jobs, 1)
  has(s.last().cmd, HOOK .. "session-start")
  has(s.last().cmd, '"session_id":"ses-1"')
  has(s.last().cmd, '"cwd":"/work"')
  has(s.last().cmd, '"source":"startup"')
  eq(s.last().opts.owner, "plugin")
end)

test("TurnStart -> user-prompt-submit", function()
  local s = setup()
  s.fire("TurnStart", { session_id = "ses-1" })
  has(s.last().cmd, HOOK .. "user-prompt-submit")
end)

test("TurnEnd -> stop", function()
  local s = setup()
  s.fire("TurnEnd", { session_id = "ses-1" })
  has(s.last().cmd, HOOK .. "stop")
end)

test("TurnError -> stop-failure with message", function()
  local s = setup()
  s.fire("TurnError", { session_id = "ses-1", message = "boom" })
  has(s.last().cmd, HOOK .. "stop-failure")
  has(s.last().cmd, '"error":"boom"')
end)

test("ToolDone -> activity-log with tool_name", function()
  local s = setup()
  s.fire("ToolDone", { session_id = "ses-1", tool = "bash", tool_id = "t1" })
  has(s.last().cmd, HOOK .. "activity-log")
  has(s.last().cmd, '"tool_name":"bash"')
end)

test("needs_input status -> notification", function()
  local s = setup()
  s.fire("SessionStatusChanged", { session_id = "ses-1", status = "needs_input" })
  has(s.last().cmd, HOOK .. "notification")
  has(s.last().cmd, '"wait_reason":"permission"')
end)

test("working status after needs_input -> user-prompt-submit", function()
  local s = setup()
  s.fire("SessionStatusChanged", { session_id = "ses-1", status = "needs_input" })
  s.fire("SessionStatusChanged", { session_id = "ses-1", status = "working" })
  has(s.last().cmd, HOOK .. "user-prompt-submit")
end)

test("other status changes ignored", function()
  local s = setup()
  local n = #s.jobs
  s.fire("SessionStatusChanged", { session_id = "ses-1", status = "idle" })
  s.fire("SessionStatusChanged", { session_id = "ses-1", status = "working" })
  eq(#s.jobs, n)
end)

test("SessionReset and SessionFocusChanged -> session-start", function()
  local s = setup()
  s.current = "ses-2"
  s.fire("SessionFocusChanged", { session_id = "ses-2" })
  has(s.last().cmd, HOOK .. "session-start")
  has(s.last().cmd, '"session_id":"ses-2"')
  s.fire("SessionReset", { session_id = "ses-2" })
  eq(#s.jobs, 3)
end)

test("events from unfocused sessions ignored", function()
  local s = setup()
  local n = #s.jobs
  s.fire("TurnEnd", { session_id = "ses-other" })
  s.fire("ToolDone", { session_id = "ses-other", tool = "bash" })
  eq(#s.jobs, n)
end)

test("single quotes in payload are shell-escaped", function()
  local s = setup()
  s.fire("TurnError", { session_id = "ses-1", message = "it's" })
  has(s.last().cmd, [[it'\''s]])
end)

test("finds hook.sh in the TPM plugin dir", function()
  local s = setup()
  local tpm = "/home/u/.tmux/plugins/tmux-agent-sidebar/hook.sh"
  _G.lain.fn.executable = function(p)
    return p == tpm and 1 or 0
  end
  package.loaded.tmux_agent_sidebar = nil
  require("tmux_agent_sidebar")
  has(s.last().cmd, "bash '" .. tpm .. "' lain session-start")
end)

test("falls back to tmux-agent-sidebar on PATH", function()
  local s = setup()
  _G.lain.fn.executable = function(p)
    return p == "tmux-agent-sidebar" and 1 or 0
  end
  package.loaded.tmux_agent_sidebar = nil
  require("tmux_agent_sidebar")
  has(s.last().cmd, "tmux-agent-sidebar hook lain session-start")
end)

test("no-op when neither hook.sh nor binary is available", function()
  local s = setup()
  local n = #s.jobs
  _G.lain.fn.executable = function()
    return 0
  end
  package.loaded.tmux_agent_sidebar = nil
  require("tmux_agent_sidebar")
  eq(#s.jobs, n)
end)

print(string.format("%d/%d passed", count - failures, count))
os.exit(failures == 0 and 0 or 1)
