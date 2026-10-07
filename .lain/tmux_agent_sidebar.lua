-- lain -> tmux-agent-sidebar bridge.
-- Install: ln -s <repo>/.lain/tmux_agent_sidebar.lua ~/.config/lain/lua/
--          and add `require("tmux_agent_sidebar")` to ~/.config/lain/init.lua.

-- The sidebar keys state off the tmux pane, so outside tmux there's nothing to do.
if not lain.uv.os_getenv("TMUX_PANE") then
  return
end

local home = lain.uv.os_homedir()
local HOOK_CANDIDATES = {
  home .. "/code/tmux-agent-sidebar/hook.sh",
  home .. "/.tmux/plugins/tmux-agent-sidebar/hook.sh",
}

local function sq(s)
  return "'" .. s:gsub("'", [['\'']]) .. "'"
end

local prefix
for _, path in ipairs(HOOK_CANDIDATES) do
  if lain.fn.executable(path) == 1 then
    prefix = "bash " .. sq(path) .. " lain "
    break
  end
end
if not prefix and lain.fn.executable("tmux-agent-sidebar") == 1 then
  prefix = "tmux-agent-sidebar hook lain "
end
if not prefix then
  return
end

-- Fire-and-forget; "plugin" owner keeps the job alive after the autocmd returns.
local function hook(event, payload)
  payload.cwd = lain.uv.cwd()
  local cmd = "printf %s " .. sq(lain.json.encode(payload)) .. " | " .. prefix .. event
  pcall(lain.fn.jobstart, cmd, { owner = "plugin" })
end

-- One pane hosts many lain sessions; mirror only the focused one.
local function focused(data)
  local current = lain.session.current()
  return current == nil or data.session_id == nil or data.session_id == current
end

local function on(event, fn)
  lain.api.create_autocmd(event, {
    callback = function(ev)
      local data = ev.data or {}
      if focused(data) then
        fn(data)
      end
    end,
  })
end

local function start(session_id)
  hook("session-start", { session_id = session_id, source = "startup" })
end

start(lain.session.current())

on("SessionReset", function(d)
  start(d.session_id)
end)
on("SessionFocusChanged", function(d)
  start(d.session_id)
end)

on("TurnStart", function(d)
  hook("user-prompt-submit", { session_id = d.session_id, prompt = "" })
end)

on("TurnEnd", function(d)
  hook("stop", { session_id = d.session_id, last_message = "" })
end)

on("TurnError", function(d)
  hook("stop-failure", { session_id = d.session_id, error = d.message or "error" })
end)

on("ToolDone", function(d)
  hook("activity-log", { session_id = d.session_id, tool_name = d.tool or "" })
end)

-- Only the needs_input edges matter; TurnStart/TurnEnd already cover working/idle.
local waiting = {}
on("SessionStatusChanged", function(d)
  local sid = d.session_id or ""
  if d.status == "needs_input" then
    waiting[sid] = true
    hook("notification", { session_id = d.session_id, wait_reason = "permission" })
  elseif waiting[sid] then
    waiting[sid] = nil
    if d.status == "working" then
      hook("user-prompt-submit", { session_id = d.session_id, prompt = "" })
    end
  end
end)
