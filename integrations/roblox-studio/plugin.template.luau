-- ChatGPT Studio Bridge: local editor commands over 127.0.0.1
assert(plugin, "Install this as a local Roblox Studio plugin")

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local History = game:GetService("ChangeHistoryService")
local ScriptEditor = game:GetService("ScriptEditorService")
local Tags = game:GetService("CollectionService")
local InsertService = game:GetService("InsertService")
local StudioTest = game:GetService("StudioTestService")
local DeviceSimulator = game:GetService("StudioDeviceSimulatorService")
local LogService = game:GetService("LogService")
local RunService = game:GetService("RunService")
local BRIDGE_VERSION = "6.0.0"
local BASE = "http://127.0.0.1:34872"
local TOKEN = "__BRIDGE_TOKEN__"
local CLIENT = HttpService:GenerateGUID(false)
local active = true
local running = true
local successfulRequests = 0
local lastAction = "No commands yet"
local simulationOwned = false
local outputBuffer = {}
local outputSequence = 0
local MAX_LOGS = 240
local logListener = LogService.MessageOut:Connect(function(message, messageType)
    outputSequence += 1
    table.insert(outputBuffer, {
        id = outputSequence,
        severity = tostring(messageType),
        text = tostring(message):sub(1, 1800),
        clock = os.time()
    })
    if #outputBuffer > MAX_LOGS then
        table.remove(outputBuffer, 1)
    end
end)

local toolbar = plugin:CreateToolbar("ChatGPT Bridge")
local toggle = toolbar:CreateButton("ChatGPTBridgePanel", "Open ChatGPT Bridge", "", "ChatGPT")
toggle.ClickableWhenViewportHidden = true

local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, true, 340, 235, 285, 185)
local widget = plugin:CreateDockWidgetPluginGuiAsync("ChatGPTStudioBridgeWidget", info)
widget.Title = "ChatGPT - Roblox Studio"

local function gui(class, name, props, parent)
    local object = Instance.new(class)
    object.Name = name
    for key, value in pairs(props or {}) do
        object[key] = value
    end
    object.Parent = parent
    return object
end

local background = gui("Frame", "Background", {
    Size = UDim2.fromScale(1, 1),
    BackgroundColor3 = Color3.fromRGB(19, 24, 33),
    BorderSizePixel = 0
}, widget)
gui("UIPadding", "Pad", {
    PaddingLeft = UDim.new(0, 18), PaddingRight = UDim.new(0, 18),
    PaddingTop = UDim.new(0, 16), PaddingBottom = UDim.new(0, 16)
}, background)
gui("UIListLayout", "Layout", {
    Padding = UDim.new(0, 9), SortOrder = Enum.SortOrder.LayoutOrder
}, background)

local function label(name, textValue, order, height, size, color)
    return gui("TextLabel", name, {
        Size = UDim2.new(1, 0, 0, height),
        BackgroundTransparency = 1,
        Text = textValue, TextColor3 = color,
        TextSize = size, Font = Enum.Font.Gotham,
        TextXAlignment = Enum.TextXAlignment.Left,
        TextWrapped = true, LayoutOrder = order
    }, background)
end
label("Title", "ChatGPT / Roblox Studio", 1, 29, 19, Color3.fromRGB(246, 248, 253))
local statusLabel = label("Status", "CONNECTING...", 2, 27, 15, Color3.fromRGB(250, 199, 98))
local detailLabel = label("Detail", "Bridge: 127.0.0.1:34872", 3, 32, 12, Color3.fromRGB(173, 185, 203))
local actionLabel = label("LastAction", "No commands yet", 4, 38, 12, Color3.fromRGB(205, 213, 225))
local pauseButton = gui("TextButton", "Pause", {
    Size = UDim2.new(1, 0, 0, 34), BackgroundColor3 = Color3.fromRGB(41, 94, 143),
    Text = "Pause connection", Font = Enum.Font.GothamBold, TextSize = 13,
    TextColor3 = Color3.fromRGB(255, 255, 255),
    BorderSizePixel = 0, LayoutOrder = 5
}, background)
gui("UICorner", "Corner", {CornerRadius = UDim.new(0, 6)}, pauseButton)

local function show(textValue, color, extra)
    statusLabel.Text = textValue
    statusLabel.TextColor3 = color
    if extra then detailLabel.Text = extra end
    actionLabel.Text = "Last: " .. lastAction
end
local green = Color3.fromRGB(115, 220, 161)
local amber = Color3.fromRGB(250, 199, 98)

toggle.Click:Connect(function()
    widget.Enabled = not widget.Enabled
end)
pauseButton.MouseButton1Click:Connect(function()
    active = not active
    pauseButton.Text = active and "Pause connection" or "Resume connection"
    if not active then
        show("PAUSED", amber, "Commands are not being accepted")
    end
end)
plugin.Unloading:Connect(function()
    running = false
    logListener:Disconnect()
end)

local function getPath(instance)
    if instance == game then return "game" end
    local names = {}
    local current = instance
    while current and current ~= game do
        table.insert(names, 1, current.Name)
        current = current.Parent
    end
    return table.concat(names, "/")
end

local function resolve(path)
    if path == "game" or path == "" or path == nil then return game end
    if type(path) ~= "string" then error("Path must be a string") end
    local parts = string.split(path, "/")
    local current = game
    local index = 1
    if parts[1] == "game" then index = 2 end
    for i = index, #parts do
        local name = parts[i]
        if name == "" then error("Empty path component") end
        local child
        if current == game then
            local ok, service = pcall(function() return game:GetService(name) end)
            if ok then child = service end
        end
        child = child or current:FindFirstChild(name)
        if not child then error("Instance not found: " .. path .. " (at " .. name .. ")") end
        current = child
    end
    return current
end

local function decodeValue(value)
    if type(value) ~= "table" or not value.__type then return value end
    local kind = value.__type
    if kind == "BrickColor" then
        return BrickColor.new(value.name or value.number)
    elseif kind == "NumberRange" then
        return NumberRange.new(value.min or 0, value.max or value.min or 0)
    elseif kind == "ColorSequence" then
        return ColorSequence.new(decodeValue(value.from), decodeValue(value.to or value.from))
    elseif kind == "NumberSequence" then
        return NumberSequence.new(value.number or 0)
    elseif kind == "UDim2" then
        return UDim2.new(value.xs or 0, value.xo or 0, value.ys or 0, value.yo or 0)
    elseif kind == "UDim" then
        return UDim.new(value.s or 0, value.o or 0)
    elseif kind == "Color3" then
        return Color3.fromRGB(value.r or 0, value.g or 0, value.b or 0)
    elseif kind == "Vector2" then
        return Vector2.new(value.x or 0, value.y or 0)
    elseif kind == "Vector3" then
        return Vector3.new(value.x or 0, value.y or 0, value.z or 0)
    elseif kind == "CFrame" then
        if value.components and #value.components == 12 then
            return CFrame.new(table.unpack(value.components))
        end
        return CFrame.new(value.x or 0, value.y or 0, value.z or 0)
            * CFrame.Angles(math.rad(value.rx or 0), math.rad(value.ry or 0), math.rad(value.rz or 0))
    elseif kind == "Rect" then
        return Rect.new(value.minX or 0, value.minY or 0, value.maxX or 0, value.maxY or 0)
    elseif kind == "Enum" then
        local group, item = string.match(value.value or "", "^Enum%.([%w_]+)%.([%w_]+)$")
        if not group then error("Invalid Enum: " .. tostring(value.value)) end
        local enumType = Enum[group]
        if not enumType then error("Unknown enum: " .. group) end
        local enumItem = enumType[item]
        if not enumItem then error("Unknown enum item: " .. item) end
        return enumItem
    elseif kind == "Ref" then
        return resolve(value.path)
    end
    error("Unsupported property type: " .. tostring(kind))
end

local function encodeValue(value)
    local kind = typeof(value)
    if kind == "nil" or kind == "boolean" or kind == "number" or kind == "string" then return value end
    if kind == "Instance" then return {__type = "Ref", path = getPath(value)} end
    if kind == "EnumItem" then return {__type = "Enum", value = tostring(value)} end
    if kind == "Color3" then
        return {__type = "Color3", r = math.floor(value.R * 255 + 0.5),
            g = math.floor(value.G * 255 + 0.5), b = math.floor(value.B * 255 + 0.5)}
    end
    if kind == "CFrame" then
        return {__type = "CFrame", components = {value:GetComponents()}}
    end
    if kind == "BrickColor" then
        return {__type = "BrickColor", name = value.Name, number = value.Number}
    end
    if kind == "NumberRange" then
        return {__type = "NumberRange", min = value.Min, max = value.Max}
    end
    if kind == "UDim2" then
        return {__type = "UDim2", xs = value.X.Scale, xo = value.X.Offset,
            ys = value.Y.Scale, yo = value.Y.Offset}
    end
    if kind == "UDim" then return {__type = "UDim", s = value.Scale, o = value.Offset} end
    if kind == "Vector3" then return {__type = "Vector3", x = value.X, y = value.Y, z = value.Z} end
    if kind == "Vector2" then return {__type = "Vector2", x = value.X, y = value.Y} end
    return tostring(value)
end


local function assertChecks(checks, root)
    if type(checks) ~= "table" or #checks < 1 or #checks > 35 then
        error("checks must contain 1..35 assertions")
    end
    local results = {}
    local pass = true
    for i, check in ipairs(checks) do
        if type(check) ~= "table" or type(check.path) ~= "string" then
            error("Invalid check at index " .. i)
        end
        local ok, found = pcall(function()
            local current = root or game
            for part in string.gmatch(check.path, "[^/]+") do
                if part ~= "game" then
                    local child = current:FindFirstChild(part)
                    if not child and current == game then
                        local good, service = pcall(function() return game:GetService(part) end)
                        if good then child = service end
                    end
                    if not child then return nil end
                    current = child
                end
            end
            return current
        end)
        local obj = ok and found or nil
        local desiredExist = check.exists
        local success = desiredExist == false and obj == nil or desiredExist ~= false and obj ~= nil
        local observed = nil
        local problem = nil
        if obj and check.exists ~= false then
            local readOK, value = pcall(function()
                if check.attribute then return obj:GetAttribute(check.attribute) end
                if check.property then
                    if check.property == "Source" then error("Source assertions prohibited; use editor source verification") end
                    return obj[check.property]
                end
                return obj.ClassName
            end)
            if not readOK then
                success = false
                problem = tostring(value)
            else
                observed = encodeValue(value)
                if check.equals ~= nil then success = observed == check.equals
                elseif check.not_equals ~= nil then success = observed ~= check.not_equals
                elseif check.contains ~= nil then
                    success = type(observed) == "string"
                        and string.find(observed, tostring(check.contains), 1, true) ~= nil
                elseif check.gt ~= nil then
                    success = type(observed) == "number" and type(check.gt) == "number" and observed > check.gt
                elseif check.lt ~= nil then
                    success = type(observed) == "number" and type(check.lt) == "number" and observed < check.lt
                end
            end
        end
        pass = pass and success
        table.insert(results, {index=i, id=tostring(check.id or i):sub(1,64),
            pass=success, path=check.path, observed=observed, error=problem})
    end
    return {passed=pass, total=#results, results=results}
end

local function setProperties(instance, props)
    if type(props) ~= "table" then error("properties must be an object") end
    local count = 0
    for property, rawValue in pairs(props) do
        if type(property) ~= "string" then error("Property name must be a string") end
        if property == "Source" then error("Use set_script for source edits") end
        local value = decodeValue(rawValue)
        instance[property] = value
        count = count + 1
    end
    return count
end

local function scriptClass(name)
    return name == "Script" or name == "LocalScript" or name == "ModuleScript"
end

local function editorSource(scriptObject)
    return ScriptEditor:GetEditorSource(scriptObject)
end

local function writeSource(scriptObject, newSource, expected)
    if #newSource > 800000 then error("Script exceeds 800 KB") end
    local attemptCount = 0
    ScriptEditor:UpdateSourceAsync(scriptObject, function(current)
        attemptCount += 1
        if expected and current ~= expected then
            error("Source changed since last read; edit rejected")
        end
        return newSource
    end)
    return attemptCount
end

local function commonServices()
    return {"Workspace", "Players", "Lighting", "ReplicatedFirst", "ReplicatedStorage",
        "ServerScriptService", "ServerStorage", "StarterGui", "StarterPack", "StarterPlayer",
        "SoundService", "Teams", "Chat", "TextChatService", "CollectionService",
        "TweenService", "RunService", "MaterialService", "LocalizationService"}
end

local function xyz(value)
    if type(value) ~= "table" then error("Expected {x,y,z}") end
    if value.__type then
        local result = decodeValue(value)
        if typeof(result) ~= "Vector3" then error("Expected Vector3") end
        return result
    end
    return Vector3.new(tonumber(value.x) or 0, tonumber(value.y) or 0, tonumber(value.z) or 0)
end

local function matches(haystack, query, caseSensitive)
    if not caseSensitive then
        haystack = string.lower(haystack)
        query = string.lower(query)
    end
    return string.find(haystack, query, 1, true) ~= nil
end


-- Studio Test Service yields until EndTest. Keep polling requests in a separate coroutine.
local studioTests = {}
local activeStudioTest = nil

local SERVER_TEST_SOURCE = [==[
local Players = game:GetService("Players")
local StudioTest = game:GetService("StudioTestService")
local HttpService = game:GetService("HttpService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local config = ReplicatedStorage:WaitForChild("__OPEN_TEST_CONFIG__")
local remote = ReplicatedStorage:WaitForChild("__OPEN_TEST_REMOTE__")
local settings = HttpService:JSONDecode(config.Value)
local events = {}
local started = os.clock()
local seen = {}
for _, player in ipairs(Players:GetPlayers()) do seen[player.Name]=true end
Players.PlayerAdded:Connect(function(player) seen[player.Name]=true end)
remote.OnServerEvent:Connect(function(player, item)
    if #events >= 150 then return end
    if type(item) ~= "table" then return end
    table.insert(events, {player=player.Name, event=tostring(item.event):sub(1,60),
        result=tostring(item.result):sub(1,300)})
end)
task.spawn(function()
    local duration = math.clamp(tonumber(settings.duration) or 10, 3, 90)
    task.wait(duration)
    local names = {}
    for _, player in ipairs(Players:GetPlayers()) do table.insert(names, player.Name) end
    local observed = {}
    for name in pairs(seen) do table.insert(observed,name) end
    local expected = settings.mode == "run" and 0 or (tonumber(settings.players) or 1)
    local passed = #observed >= expected
    for _, item in ipairs(events) do
        if item.event == "input_error" or item.event == "ui_missing" then passed = false end
    end
    for index, check in ipairs(settings.checks or {}) do
        if check.scope == "server" then
            local ok, success, value = pcall(function()
                local parts=string.split(check.path, "/")
                local instance=game
                for _, part in ipairs(parts) do
                    if part~="game" then
                        local nextNode = instance:FindFirstChild(part)
                        if not nextNode and instance==game then
                            local good, svc=pcall(function() return game:GetService(part) end)
                            if good then nextNode=svc end
                        end
                        instance=nextNode
                        if not instance then break end
                    end
                end
                if check.exists==false then return instance==nil, instance and "present" or "absent" end
                if not instance then return false, "not found" end
                local raw
                if check.attribute then raw=instance:GetAttribute(check.attribute)
                elseif check.property then raw=instance[check.property]
                else raw=instance.ClassName end
                local matched=true
                if check.equals ~= nil then matched=raw==check.equals
                elseif check.not_equals ~= nil then matched=raw~=check.not_equals
                elseif check.contains ~= nil then
                    matched=type(raw)=="string" and raw:find(check.contains,1,true)~=nil
                elseif check.gt ~= nil then matched=type(raw)=="number" and raw>check.gt
                elseif check.lt ~= nil then matched=type(raw)=="number" and raw<check.lt end
                return matched, tostring(raw)
            end)
            local valid = ok and success
            if not valid then passed=false end
            table.insert(events, {player="server", event=valid and "assert_pass" or "assert_fail",
                result=tostring(check.id or index):sub(1,60) .. ":" .. tostring(ok and value or success):sub(1,180)})
        end
    end
    local completed={}
    for _,item in ipairs(events) do
        if item.event=="client_done" then completed[item.player]=true end
        if item.event=="assert_fail" then passed=false end
    end
    if expected>0 then
        local complete=0
        for _ in pairs(completed) do complete+=1 end
        if complete<expected then passed=false end
    end
    local result = {success=passed, players=#observed, expectedPlayers=expected,
        playerNames=observed, livePlayers=#names,
        events=events, duration=os.clock()-started, mode=tostring(settings.mode),
        note="Server harness result. Client telemetry is best-effort."}
    local ok, err = pcall(function() StudioTest:EndTest(result) end)
    if not ok then warn("OPEN_BRIDGE_TEST_END_ERROR " .. tostring(err)) end
end)
]==]

local CLIENT_TEST_SOURCE = [==[
local Players = game:GetService("Players")
local UIS = game:GetService("UserInputService")
local HttpService = game:GetService("HttpService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local config = ReplicatedStorage:WaitForChild("__OPEN_TEST_CONFIG__")
local remote = ReplicatedStorage:WaitForChild("__OPEN_TEST_REMOTE__")
local settings = HttpService:JSONDecode(config.Value)
local player = Players.LocalPlayer
if not player then return end
local function report(event, result)
    pcall(function() remote:FireServer({event=event, result=tostring(result)}) end)
end
task.wait(1.5)
report("client_ready", tostring(player.Name))
local gui = player:WaitForChild("PlayerGui", 7)
if gui then
    local ready = false
    for _ = 1, 12 do
        for _, node in ipairs(gui:GetDescendants()) do
            if node:IsA("GuiObject") then ready = true break end
        end
        if ready or not (settings.expectedUiMin and settings.expectedUiMin > 0) then
            break
        end
        task.wait(0.45)
    end
    local checked, outside = 0, 0
    for _, node in ipairs(gui:GetDescendants()) do
        if node:IsA("GuiObject") and node.Visible and checked < 150 then
            checked += 1
            local pos, size = node.AbsolutePosition, node.AbsoluteSize
            local camera = workspace.CurrentCamera
            local viewport = camera and camera.ViewportSize or Vector2.new(0,0)
            if viewport.X > 0 and viewport.Y > 0 and size.X > 1 and size.Y > 1 then
                if pos.X + size.X < 0 or pos.Y + size.Y < 0 or pos.X > viewport.X or pos.Y > viewport.Y then
                    outside += 1
                    report("gui_outside", node:GetFullName())
                end
            end
        end
    end
    report("ui_audit", "checked=" .. checked .. ";outside=" .. outside)
    if checked < (tonumber(settings.expectedUiMin) or 0) then
        report("ui_missing", "expected=" .. tostring(settings.expectedUiMin) .. ";observed=" .. checked)
    end
else
    report("ui_missing", "PlayerGui did not load")
end
local steps = settings.steps or {}
if #steps > 0 then
    local ok, vi = pcall(function() return UIS:CreateVirtualInput() end)
    if not ok or not vi then
        report("input_error", "VirtualInput unavailable: " .. tostring(vi))
    else
        for i, step in ipairs(steps) do
            task.wait(math.clamp(tonumber(step.delay) or 0.1, 0, 3))
            local worked, err = pcall(function()
                if step.type == "text" then
                    vi:SendTextInput(tostring(step.text):sub(1,120))
                elseif step.type == "key" then
                    local key = Enum.KeyCode[tostring(step.key)]
                    if not key then error("Invalid key") end
                    vi:SendKey(true, key, false)
                    task.wait(0.09)
                    vi:SendKey(false, key, false)
                elseif step.type == "click" then
                    local pt = Vector2.new(tonumber(step.x) or 0, tonumber(step.y) or 0)
                    vi:SendMousePosition(pt)
                    vi:SendMouseButton(pt, Enum.UserInputType.MouseButton1, true, 0)
                    task.wait(0.09)
                    vi:SendMouseButton(pt, Enum.UserInputType.MouseButton1, false, 0)
                else
                    error("Unsupported input step")
                end
            end)
            report(worked and "input_ok" or "input_error", tostring(i) .. ":" .. tostring(err))
        end
    end
end
local function checkClient(item)
    local root = item.path:match("^([^/]+)") or ""
    local current
    if root == "PlayerGui" then current = player:FindFirstChildOfClass("PlayerGui")
    elseif root == "Workspace" then current = workspace
    elseif root == "ReplicatedStorage" then current = ReplicatedStorage
    else return false, "Unsupported client root" end
    for segment in item.path:gmatch("/([^/]+)") do
        current = current and current:FindFirstChild(segment)
    end
    if item.exists == false then return current == nil, current and "present" or "absent" end
    if not current then return false, "not found" end
    local ok, observed = pcall(function()
        if item.attribute then return current:GetAttribute(item.attribute) end
        return item.property and current[item.property] or current.ClassName
    end)
    if not ok then return false, tostring(observed) end
    local passed = true
    if item.equals ~= nil then passed = observed == item.equals
    elseif item.not_equals ~= nil then passed = observed ~= item.not_equals
    elseif item.contains ~= nil then
        passed = type(observed)=="string" and observed:find(item.contains,1,true) ~= nil
    elseif item.gt ~= nil then passed=type(observed)=="number" and observed > item.gt
    elseif item.lt ~= nil then passed=type(observed)=="number" and observed < item.lt end
    return passed, tostring(observed)
end
for i, item in ipairs(settings.checks or {}) do
    if item.scope == "client" then
        task.wait(math.clamp(tonumber(item.delay) or 0.15, 0, 2))
        local ok, passed, observed = pcall(checkClient, item)
        report(ok and passed and "assert_pass" or "assert_fail",
            tostring(item.id or i) .. ":" .. tostring(ok and observed or passed):sub(1,160))
    end
end
report("client_done", player.Name)
]==]

local function cleanupTestParts(names)
    for _, item in ipairs(names) do
        pcall(function() if item and item.Parent then item:Destroy() end end)
    end
end

local function prepareStudioTest(payload)
    local serverRoot = game:GetService("ServerScriptService")
    local clientRoot = game:GetService("StarterPlayer"):WaitForChild("StarterPlayerScripts")
    local replicated = game:GetService("ReplicatedStorage")
    local created = {}
    local ok, err = pcall(function()
        for _, entry in ipairs({
            {parent=replicated, class="StringValue", name="__OPEN_TEST_CONFIG__"},
            {parent=replicated, class="RemoteEvent", name="__OPEN_TEST_REMOTE__"},
            {parent=serverRoot, class="Script", name="__OPEN_TEST_SERVER__"},
            {parent=clientRoot, class="LocalScript", name="__OPEN_TEST_CLIENT__"}
        }) do
            if entry.parent:FindFirstChild(entry.name) then
                error("Reserved test item already present: " .. entry.name)
            end
            local obj=Instance.new(entry.class)
            obj.Name=entry.name
            obj.Parent=entry.parent
            table.insert(created,obj)
        end
        created[1].Value=HttpService:JSONEncode(payload)
        ScriptEditor:UpdateSourceAsync(created[3],function() return SERVER_TEST_SOURCE end)
        ScriptEditor:UpdateSourceAsync(created[4],function() return CLIENT_TEST_SOURCE end)
    end)
    if not ok then cleanupTestParts(created); error(err) end
    return created
end

local function beginStudioTest(command)
    if activeStudioTest then error("A Studio test is already active") end
    if not StudioTest.EditModeActive then error("Studio must be in Edit mode") end
    if command.confirm_persistent_changes ~= true then
        error("Confirm test in a saved disposable place: confirm_persistent_changes=true")
    end
    local mode = command.mode or "play"
    if mode ~= "play" and mode ~= "multiplayer" and mode ~= "run" then error("Unknown test mode") end
    local players = math.floor(tonumber(command.players) or 2)
    if players < 1 or players > 8 then error("players must be 1..8") end
    local duration = math.clamp(tonumber(command.duration) or (mode=="multiplayer" and 24 or 9),
        mode=="multiplayer" and 18 or 3, 90)
    local steps = command.steps or {}
    if type(steps) ~= "table" or #steps > 25 then error("steps supports at most 25 actions") end
    for _, step in ipairs(steps) do
        if type(step) ~= "table" or (step.type ~= "key" and step.type ~= "click" and step.type ~= "text") then
            error("Unsupported input step; use type key/click/text")
        end
        if step.type == "click" and (type(step.x) ~= "number" or type(step.y) ~= "number" or
            step.x < 0 or step.y < 0 or step.x > 7680 or step.y > 4320) then
            error("Invalid click coordinates")
        end
        if step.type == "text" and (type(step.text) ~= "string" or #step.text > 120) then
            error("Text step limited to 120 characters")
        end
    end
    local checks = command.checks or {}
    if type(checks) ~= "table" or #checks > 25 then error("checks must contain up to 25 entries") end
    for i, check in ipairs(checks) do
        if type(check) ~= "table" or type(check.path) ~= "string" or #check.path > 300
            or (check.scope ~= "client" and check.scope ~= "server") then
            error("Invalid runtime assertion at " .. i)
        end
        if check.property == "Source" then error("Source checks prohibited") end
    end
    local id = HttpService:GenerateGUID(false)
    local expectedUiMin=math.clamp(math.floor(tonumber(command.expected_ui_min) or 0),0,150)
    local payload = {mode=mode, duration=duration, steps=steps, checks=checks, id=id,
        expectedUiMin=expectedUiMin,
        players=mode=="multiplayer" and players or (mode=="run" and 0 or 1)}
    local parts=prepareStudioTest(payload)
    local record = {id=id, state="running", mode=mode, players=mode=="multiplayer" and players or 1,
        created=os.time(), duration=duration}
    studioTests[id]=record
    activeStudioTest=id
    task.spawn(function()
        local ok, result=pcall(function()
            if mode=="multiplayer" then
                return StudioTest:ExecuteMultiplayerTestAsync(players,payload)
            elseif mode=="run" then
                return StudioTest:ExecuteRunModeAsync(payload)
            else
                return StudioTest:ExecutePlayModeAsync(payload)
            end
        end)
        record.state=ok and "done" or "failed"
        record.result=ok and result or nil
        record.error=not ok and tostring(result) or nil
        record.finished=os.time()
        cleanupTestParts(parts)
        activeStudioTest=nil
    end)
    return {id=id, state="running", mode=mode, duration=duration,
        warning="Test may temporarily interrupt Studio plugin polling while player windows start."}
end

local function studioTestStatus(id)
    local record=studioTests[id or activeStudioTest]
    if not record then return {active=activeStudioTest, error="Test not found"} end
    return {id=record.id, state=record.state, mode=record.mode, players=record.players,
        duration=record.duration, result=record.result, error=record.error, finished=record.finished}
end

local function handle(command, insideBatch)
    if type(command) ~= "table" or type(command.op) ~= "string" then error("Missing op") end
    local op = command.op
    if op == "ping" then
        return {pong = true, place = game.Name, placeId = game.PlaceId, client = CLIENT, version = BRIDGE_VERSION}
    elseif op == "suite_manifest" then
        local nodes = {}
        local maxNodes = math.clamp(tonumber(command.limit) or 700, 1, 1000)
        local maxSource = math.clamp(tonumber(command.source_limit) or 5000, 0, 12000)
        local roots = {"ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui",
            "StarterPlayer", "StarterPack", "ReplicatedFirst", "SoundService", "Lighting",
            "Teams", "Chat", "Workspace"}
        local sourceBytes = 0
        local scripts, remotes, ui = 0, 0, 0
        local truncated = false
        for _, name in ipairs(roots) do
            local ok, root = pcall(function() return game:GetService(name) end)
            if ok then
                for _, node in ipairs(root:GetDescendants()) do
                    if #nodes >= maxNodes then truncated=true break end
                    local item = {path=getPath(node), name=node.Name, class=node.ClassName}
                    if node:IsA("LuaSourceContainer") then
                        scripts += 1
                        local readOK, source = pcall(editorSource, node)
                        if readOK then
                            local available = math.max(0, 380000 - sourceBytes)
                            local length = math.min(maxSource, available)
                            if length > 0 then
                                item.source = string.sub(source, 1, length)
                                sourceBytes += #(item.source)
                            end
                            item.sourceTruncated = #source > length
                        end
                    end
                    if node:IsA("RemoteEvent") or node:IsA("RemoteFunction") then remotes += 1 end
                    if node:IsA("GuiObject") then
                        ui += 1
                        item.visible=node.Visible
                        item.sizeX=node.AbsoluteSize.X
                        item.sizeY=node.AbsoluteSize.Y
                        if node:IsA("TextLabel") or node:IsA("TextButton") or node:IsA("TextBox") then
                            item.text=string.sub(node.Text,1,160)
                            item.textSize=node.TextSize
                        end
                    end
                    table.insert(nodes,item)
                end
            end
            if truncated then break end
        end
        return {version=BRIDGE_VERSION, nodes=nodes, total=#nodes, scripts=scripts,
            remotes=remotes, ui=ui, truncated=truncated, sourceBytes=sourceBytes,
            note="Bounded editor snapshot; some scripts may be truncated or inaccessible."}
    elseif op == "suite_perf" then
        local Stats = game:GetService("Stats")
        local count, scripts, parts, ui = 0, 0, 0, 0
        for _, node in ipairs(game:GetDescendants()) do
            count+=1
            if node:IsA("LuaSourceContainer") then scripts+=1 end
            if node:IsA("BasePart") then parts+=1 end
            if node:IsA("GuiObject") then ui+=1 end
        end
        local ok, memory=pcall(function() return Stats:GetTotalMemoryUsageMb() end)
        return {objects=count, scripts=scripts, parts=parts, ui=ui,
            memoryMb=ok and memory or nil, running=RunService:IsRunning(),
            note="Studio snapshot only, not FPS/latency profiling or a production performance benchmark."}
    elseif op == "assert_state" then
        return assertChecks(command.checks)
    elseif op == "run_state" then
        return {
            isEdit = RunService:IsEdit(),
            isRunning = RunService:IsRunning(),
            isRunMode = RunService:IsRunMode(),
            simulationOwned = simulationOwned,
            version = BRIDGE_VERSION
        }
    elseif op == "logs" or op == "diagnostics" then
        local since = math.max(0, tonumber(command.since) or 0)
        local limit = math.clamp(tonumber(command.limit) or 60, 1, 120)
        local query = tostring(command.query or ""):lower()
        local severity = tostring(command.severity or ""):lower()
        local matchesLog = {}
        local errorCount, warningCount = 0, 0
        for _, entry in ipairs(outputBuffer) do
            local level = entry.severity:lower()
            if level:find("error", 1, true) then errorCount += 1 end
            if level:find("warn", 1, true) then warningCount += 1 end
            if entry.id > since and (severity == "" or level:find(severity, 1, true))
                and (query == "" or entry.text:lower():find(query, 1, true)) then
                table.insert(matchesLog, entry)
            end
        end
        local truncated = #matchesLog > limit
        while #matchesLog > limit do table.remove(matchesLog, 1) end
        return {
            version = BRIDGE_VERSION,
            logs = matchesLog,
            count = #matchesLog,
            truncated = truncated,
            oldestAvailable = #outputBuffer > 0 and outputBuffer[1].id or 0,
            nextSince = outputSequence,
            errors = errorCount,
            warnings = warningCount,
            isRunning = RunService:IsRunning(),
            isEdit = RunService:IsEdit(),
            note = "Only messages observed by this Studio plugin are included. Client/output messages from other contexts might be missing."
        }
    elseif op == "logs_clear" then
        table.clear(outputBuffer)
        return {cleared = true, nextSince = outputSequence}
    elseif op == "test_run" then
        if simulationOwned or RunService:IsRunning() then error("Test simulation already active; check run_state") end
        if command.confirm_persistent_changes ~= true then
            error("RunService Stop does not restore the pretest world. Set confirm_persistent_changes=true after saving an expendable test place.")
        end
        RunService:Run()
        simulationOwned = true
        return {started = true, isRunning = RunService:IsRunning(), mode = "simulation_no_player"}
    elseif op == "test_pause" then
        if not RunService:IsRunning() then error("Simulation is not running") end
        RunService:Pause()
        return {paused = true}
    elseif op == "test_resume" then
        if RunService:IsRunning() then error("Simulation is already running") end
        if not simulationOwned then error("No owned paused simulation; use test_run") end
        RunService:Run()
        return {resumed = true}
    elseif op == "test_stop" then
        if not simulationOwned and not RunService:IsRunning() then error("No simulation to stop") end
        if command.confirm_persistent_changes ~= true then
            error("RunService:Stop preserves simulation changes. Set confirm_persistent_changes=true on an expendable test place.")
        end
        RunService:Stop()
        simulationOwned = false
        return {stopped = true, worldRestored = false}
    elseif op == "playtest_start" then
        return beginStudioTest(command)
    elseif op == "playtest_status" then
        return studioTestStatus(command.id)
    elseif op == "ui_scan" then
        local root=resolve(command.root or "StarterGui")
        local limit=math.clamp(tonumber(command.limit) or 80,1,150)
        local items={}
        for _, node in ipairs(root:GetDescendants()) do
            if node:IsA("GuiObject") then
                local pos, size=node.AbsolutePosition,node.AbsoluteSize
                table.insert(items,{path=getPath(node),class=node.ClassName,visible=node.Visible,
                    position=encodeValue(pos),size=encodeValue(size)})
            end
            if #items>=limit then break end
        end
        local camera=workspace.CurrentCamera
        return {root=getPath(root),count=#items,items=items,
            viewport=camera and encodeValue(camera.ViewportSize) or nil,
            note="Edit-mode layout may differ from each player's PlayerGui. Use playtest with expected_ui_min for client assertions."}
    elseif op == "device_list" then
        local ids=DeviceSimulator:GetDeviceListAsync()
        local output={}
        for i, id in ipairs(ids) do
            if i>45 then break end
            local ok, info=pcall(function() return DeviceSimulator:GetDeviceInfoAsync(id) end)
            table.insert(output,{id=id, name=ok and info.Name or id, width=ok and info.Width or nil,
                height=ok and info.Height or nil, form=ok and tostring(info.DeviceForm) or nil})
        end
        return {current=DeviceSimulator:GetDeviceAsync(),devices=output}
    elseif op == "device_status" then
        local id=DeviceSimulator:GetDeviceAsync()
        local result={device=id}
        if id~="default" then
            result.resolution=encodeValue(DeviceSimulator:GetResolutionAsync())
            result.orientation=tostring(DeviceSimulator:GetOrientationAsync())
            result.density=DeviceSimulator:GetPixelDensityAsync()
        end
        return result
    elseif op == "device_select" then
        if type(command.device_id)~="string" then error("device_id required") end
        DeviceSimulator:SetDeviceAsync(command.device_id)
        return {device=DeviceSimulator:GetDeviceAsync()}
    elseif op == "device_resolution" then
        local w,h=tonumber(command.width),tonumber(command.height)
        if not w or not h or w<1 or h<1 or w>7680 or h>4320 then error("Invalid device resolution") end
        DeviceSimulator:SetResolutionAsync(math.floor(w),math.floor(h))
        return {resolution=encodeValue(DeviceSimulator:GetResolutionAsync())}
    elseif op == "device_orientation" then
        local mode=command.orientation
        local item=Enum.ScreenOrientation[mode]
        if not item or (mode~="Portrait" and mode~="LandscapeLeft" and mode~="LandscapeRight") then
            error("orientation must be Portrait, LandscapeLeft, or LandscapeRight")
        end
        DeviceSimulator:SetOrientationAsync(item)
        return {orientation=tostring(DeviceSimulator:GetOrientationAsync())}
    elseif op == "device_stop" then
        DeviceSimulator:StopSimulationAsync()
        return {device=DeviceSimulator:GetDeviceAsync()}
    elseif op == "capabilities" or op == "project_info" then
        local services = {}
        for _, name in ipairs(commonServices()) do
            local ok, service = pcall(function() return game:GetService(name) end)
            if ok then
                table.insert(services, {name = name, class = service.ClassName, children = #service:GetChildren()})
            end
        end
        return {version = BRIDGE_VERSION, place = game.Name, placeId = game.PlaceId,
            gameId = game.GameId, services = services,
            operations = {"ping", "capabilities", "project_info", "tree", "find", "selection",
                "properties", "get_attributes", "tags", "read_script", "script_search", "create",
                "update", "set_script", "replace_script", "open_script", "delete", "move", "duplicate",
                "select", "set_attributes", "set_tags", "pivot", "terrain_fill", "terrain_clear",
                "insert_asset", "batch", "undo", "redo", "run_state", "logs", "diagnostics", "logs_clear", "test_run", "test_pause", "test_resume", "test_stop", "playtest_start", "playtest_status", "ui_scan", "assert_state", "suite_manifest", "suite_perf", "device_list", "device_status", "device_select", "device_resolution", "device_orientation", "device_stop"},
            note = "Edit-mode data model only; saving, publishing and gameplay tests use Studio UI."}
    elseif op == "find" then
        local root = resolve(command.root or "game")
        local limit = math.clamp(tonumber(command.limit) or 100, 1, 300)
        local query = tostring(command.query or "")
        local classFilter = command.class
        local hits = {}
        local scanned = 0
        local nodes = root:GetDescendants()
        for _, node in ipairs(nodes) do
            scanned += 1
            local classOK = not classFilter or node:IsA(classFilter)
            if classOK and matches(node.Name, query, command.case_sensitive) then
                table.insert(hits, {path = getPath(node), class = node.ClassName})
                if #hits >= limit then break end
            end
        end
        return {items = hits, matched = #hits, scanned = scanned, truncated = #hits >= limit}
    elseif op == "get_attributes" then
        local obj = resolve(command.path)
        local output = {}
        for name, value in pairs(obj:GetAttributes()) do output[name] = encodeValue(value) end
        return {path = getPath(obj), attributes = output}
    elseif op == "tags" then
        local obj = resolve(command.path)
        local found = Tags:GetTags(obj)
        table.sort(found)
        return {path = getPath(obj), tags = found}
    elseif op == "script_search" then
        local root = resolve(command.root or "game")
        local query = command.query
        if type(query) ~= "string" or #query == 0 then error("query required") end
        local results = {}
        local maxResults = math.clamp(tonumber(command.limit) or 50, 1, 150)
        for _, obj in ipairs(root:GetDescendants()) do
            if obj:IsA("LuaSourceContainer") then
                local ok, source = pcall(editorSource, obj)
                if ok and matches(source, query, command.case_sensitive) then
                    local target = command.case_sensitive and source or string.lower(source)
                    local needle = command.case_sensitive and query or string.lower(query)
                    local pos = string.find(target, needle, 1, true)
                    table.insert(results, {path = getPath(obj), class = obj.ClassName,
                        position = pos, preview = string.sub(source, math.max(1, pos - 75), math.min(#source, pos + #query + 75))})
                    if #results >= maxResults then break end
                end
            end
        end
        return {matches = results, count = #results, truncated = #results >= maxResults}
    elseif op == "selection" then
        local paths = {}
        for _, obj in ipairs(Selection:Get()) do table.insert(paths, getPath(obj)) end
        return {paths = paths}
    elseif op == "tree" then
        local root = resolve(command.root or "game")
        local depth = math.clamp(tonumber(command.depth) or 2, 0, 5)
        local limit = math.clamp(tonumber(command.limit) or 120, 1, 300)
        local visited = 0
        local function walk(instance, level)
            visited = visited + 1
            local node = {path = getPath(instance), class = instance.ClassName, name = instance.Name}
            if level < depth and visited < limit then
                node.children = {}
                local children = instance:GetChildren()
                table.sort(children, function(a, b) return a.Name < b.Name end)
                for _, child in ipairs(children) do
                    if visited >= limit then break end
                    table.insert(node.children, walk(child, level + 1))
                end
            end
            return node
        end
        local data = walk(root, 0)
        return {tree = data, count = visited, truncated = visited >= limit}
    elseif op == "properties" then
        local obj = resolve(command.path)
        local defaults = {"Name", "Archivable", "Enabled", "Visible", "Text", "Size",
            "Position", "AnchorPoint", "BackgroundColor3", "TextColor3",
            "BackgroundTransparency", "Transparency", "Color", "Material",
            "CanCollide", "Anchored", "ResetOnSpawn", "ZIndex", "Parent"}
        local names = command.names or defaults
        if type(names) ~= "table" then error("names must be array") end
        local values = {}
        for i = 1, math.min(#names, 45) do
            local name = names[i]
            if name ~= "Source" then
                local ok, value = pcall(function() return obj[name] end)
                if ok then values[name] = encodeValue(value) end
            end
        end
        return {path = getPath(obj), class = obj.ClassName, properties = values}
    elseif op == "read_script" then
        local obj = resolve(command.path)
        if not obj:IsA("LuaSourceContainer") then error("Not a script: " .. getPath(obj)) end
        return {path = getPath(obj), class = obj.ClassName, source = editorSource(obj)}
    elseif op == "create" then
        local parent = resolve(command.parent)
        if not command.class or not command.name then error("create needs class, name, parent") end
        if string.find(command.name, "/", 1, true) then error("Slash not supported in names") end
        local previous = parent:FindFirstChild(command.name)
        if previous then
            if command.on_conflict == "update" and previous.ClassName == command.class then
                setProperties(previous, command.properties or {})
                return {path = getPath(previous), updated = true}
            end
            error("Instance already exists: " .. getPath(previous))
        end
        local obj = Instance.new(command.class)
        obj.Name = command.name
        setProperties(obj, command.properties or {})
        obj.Parent = parent
        return {path = getPath(obj), class = obj.ClassName, created = true}
    elseif op == "update" then
        local obj = resolve(command.path)
        if obj == game then error("Cannot edit game root") end
        local count = setProperties(obj, command.properties)
        return {path = getPath(obj), propertiesSet = count}
    elseif op == "set_script" then
        if type(command.source) ~= "string" then error("set_script requires source string") end
        local obj
        local found = pcall(function() obj = resolve(command.path) end)
        if not found then
            local parentPath, name = string.match(command.path or "", "^(.*)/([^/]+)$")
            if command.parent then
                parentPath = command.parent
                name = command.name or command.path
            end
            if not parentPath or not name or name == "" then error("Supply full script path or parent + name") end
            local parent = resolve(parentPath)
            if parent:FindFirstChild(name) then error("Script lookup failed on an existing object") end
            local className = command.class or "ModuleScript"
            if not scriptClass(className) then error("Invalid script class") end
            obj = Instance.new(className)
            obj.Name = name
            obj.Parent = parent
        end
        if not obj:IsA("LuaSourceContainer") then error("Target is not a Luau script") end
        local attempts = writeSource(obj, command.source, command.expected_source)
        return {path = getPath(obj), class = obj.ClassName, bytes = #command.source, attempts = attempts}
    elseif op == "replace_script" then
        local obj = resolve(command.path)
        if not obj:IsA("LuaSourceContainer") then error("Target is not a Luau script") end
        local oldText = command.search
        local newText = command.replacement
        if type(oldText) ~= "string" or #oldText == 0 or type(newText) ~= "string" then
            error("search and replacement are required strings")
        end
        local current = editorSource(obj)
        if command.expected_source and current ~= command.expected_source then error("Script changed") end
        local locations = {}
        local offset = 1
        while true do
            local at = string.find(current, oldText, offset, true)
            if not at then break end
            table.insert(locations, at)
            offset = at + #oldText
        end
        if #locations == 0 then error("Search text not found") end
        local replacement = string.gsub(current, oldText:gsub("([^%w])", "%%%1"), newText:gsub("%%", "%%%%"), command.all and #locations or 1)
        writeSource(obj, replacement, current)
        return {path = getPath(obj), replaced = command.all and #locations or 1, bytes = #replacement}
    elseif op == "open_script" then
        local obj = resolve(command.path)
        if not obj:IsA("LuaSourceContainer") then error("Target is not a Luau script") end
        ScriptEditor:OpenScriptDocumentAsync(obj)
        return {opened = getPath(obj)}
    elseif op == "set_attributes" then
        local obj = resolve(command.path)
        if type(command.attributes) ~= "table" then error("attributes object required") end
        local count = 0
        for name, raw in pairs(command.attributes) do
            if type(name) ~= "string" then error("Invalid attribute name") end
            obj:SetAttribute(name, decodeValue(raw))
            count += 1
        end
        return {path = getPath(obj), count = count}
    elseif op == "set_tags" then
        local obj = resolve(command.path)
        local changes = 0
        for _, tag in ipairs(command.add or {}) do Tags:AddTag(obj, tag) changes += 1 end
        for _, tag in ipairs(command.remove or {}) do Tags:RemoveTag(obj, tag) changes += 1 end
        return {path = getPath(obj), tags = Tags:GetTags(obj), count = changes}
    elseif op == "pivot" then
        local obj = resolve(command.path)
        local value = decodeValue(command.cframe)
        if typeof(value) ~= "CFrame" then error("pivot requires typed CFrame") end
        if obj:IsA("PVInstance") then obj:PivotTo(value)
        elseif obj:IsA("BasePart") then obj.CFrame = value
        else error("pivot requires Model or BasePart") end
        return {path = getPath(obj), cframe = encodeValue(obj:GetPivot())}
    elseif op == "terrain_fill" or op == "terrain_clear" then
        local terrain = workspace.Terrain
        local shape = command.shape or "block"
        local mat
        if op == "terrain_clear" then mat = Enum.Material.Air
        else
            local name = command.material or "Grass"
            mat = Enum.Material[name]
            if not mat then error("Invalid terrain material") end
        end
        local center = xyz(command.center)
        if shape == "ball" then
            local radius = tonumber(command.radius)
            if not radius or radius <= 0 or radius > 1024 then error("radius must be 0..1024") end
            terrain:FillBall(center, radius, mat)
        elseif shape == "block" then
            local size = xyz(command.size)
            if size.X <= 0 or size.Y <= 0 or size.Z <= 0 or size.Magnitude > 3000 then error("Invalid size") end
            terrain:FillBlock(CFrame.new(center), size, mat)
        elseif shape == "cylinder" then
            local radius = tonumber(command.radius)
            local height = tonumber(command.height)
            if not radius or not height or radius <= 0 or radius > 1024 or height <= 0 or height > 2048 then error("Invalid cylinder") end
            terrain:FillCylinder(CFrame.new(center), height, radius, mat)
        else error("Supported shapes: block, ball, cylinder") end
        return {shape = shape, material = mat.Name, center = encodeValue(center)}
    elseif op == "insert_asset" then
        local assetId = tonumber(command.asset_id)
        if not assetId or assetId <= 0 or assetId % 1 ~= 0 then error("Positive asset_id required") end
        local parent = resolve(command.parent or "Workspace")
        local pack = InsertService:LoadAsset(assetId)
        pack.Parent = parent
        if command.name then pack.Name = command.name end
        return {path = getPath(pack), children = #pack:GetChildren()}
    elseif op == "delete" then
        local obj = resolve(command.path)
        if obj == game or obj.Parent == game then error("Cannot delete game or top-level services") end
        local path = getPath(obj)
        obj:Destroy()
        return {deleted = path}
    elseif op == "move" then
        local obj = resolve(command.path)
        if obj == game or obj.Parent == game then error("Cannot move root or service") end
        local target = resolve(command.parent)
        if target == obj or target:IsDescendantOf(obj) then error("Cannot parent inside itself") end
        obj.Parent = target
        return {path = getPath(obj)}
    elseif op == "duplicate" then
        local obj = resolve(command.path)
        if obj == game or obj.Parent == game then error("Cannot duplicate root or service") end
        local target = resolve(command.parent or getPath(obj.Parent))
        local copy = obj:Clone()
        if not copy then error("Object is not cloneable") end
        copy.Name = command.name or (obj.Name .. "Copy")
        if target:FindFirstChild(copy.Name) then error("Duplicate name in target") end
        copy.Parent = target
        return {path = getPath(copy)}
    elseif op == "select" then
        if type(command.paths) ~= "table" then error("select requires paths") end
        local paths = {}
        local objects = {}
        for i, path in ipairs(command.paths) do
            if i > 100 then break end
            local obj = resolve(path)
            table.insert(objects, obj)
            table.insert(paths, getPath(obj))
        end
        Selection:Set(objects)
        return {selected = paths}
    elseif op == "batch" then
        if insideBatch then error("Nested batches not supported") end
        if type(command.actions) ~= "table" or #command.actions > 40 then
            error("batch needs an array of up to 40 actions")
        end
        local results = {}
        for i, action in ipairs(command.actions) do
            if type(action) ~= "table" or action.op == "batch" or action.op == "undo" or action.op == "redo" then
                error("Invalid batch action at index " .. i)
            end
            local ok, result = pcall(handle, action, true)
            if not ok then error("Batch action " .. i .. " failed: " .. tostring(result)) end
            table.insert(results, result)
        end
        return {results = results, count = #results}
    elseif op == "undo" or op == "redo" then
        if insideBatch then error("Undo/redo not allowed inside batch") end
        local can
        if op == "undo" then can = History:GetCanUndo() else can = History:GetCanRedo() end
        if not can then error("No history item to " .. op) end
        if op == "undo" then History:Undo() else History:Redo() end
        return {action = op}
    end
    error("Unknown command op: " .. op)
end

local function execute(command)
    local readOnly = {ping = true, capabilities = true, project_info = true,
        tree = true, find = true, selection = true, properties = true,
        get_attributes = true, tags = true, read_script = true, script_search = true,
        run_state = true, logs = true, diagnostics = true, logs_clear = true,
        playtest_start = true, playtest_status = true, ui_scan = true, assert_state = true, suite_manifest = true, suite_perf = true, device_list = true, device_status = true,
        device_select = true, device_resolution = true, device_orientation = true, device_stop = true,
        test_run = true, test_pause = true, test_resume = true, test_stop = true,
        open_script = true, undo = true, redo = true}
    if readOnly[command.op] then return handle(command, false) end
    local recording = History:TryBeginRecording("ChatGPTBridge:" .. tostring(command.op), "ChatGPT " .. tostring(command.op))
    if not recording then error("Cannot begin Studio Undo recording. Stop playtest or finish another plugin edit.") end
    local ok, result = pcall(handle, command, false)
    if ok then
        History:FinishRecording(recording, Enum.FinishRecordingOperation.Commit)
        return result
    end
    History:FinishRecording(recording, Enum.FinishRecordingOperation.Cancel)
    error(result)
end

local function http(method, route, body)
    local requestOptions = {
        Url = BASE .. route, Method = method,
        Headers = {
            ["X-Bridge-Token"] = TOKEN,
            ["X-Bridge-Client"] = CLIENT,
            ["X-Bridge-Place"] = tostring(game.PlaceId)
        }
    }
    if body ~= nil then
        requestOptions.Headers["Content-Type"] = "application/json"
        requestOptions.Body = HttpService:JSONEncode(body)
    end
    local response = HttpService:RequestAsync(requestOptions)
    if not response.Success then
        error("HTTP " .. tostring(response.StatusCode) .. ": " .. tostring(response.Body))
    end
    return HttpService:JSONDecode(response.Body)
end

task.spawn(function()
    local pendingResult
    while running do
        if active then
            if pendingResult then
                local ok, err = pcall(function()
                    http("POST", "/studio/result", pendingResult)
                end)
                if ok then
                    pendingResult = nil
                else
                    show("RESULT RETRY", amber, tostring(err):sub(1, 120))
                end
            else
                local ok, message = pcall(function()
                    local nextJob = http("GET", "/studio/next")
                    successfulRequests = successfulRequests + 1
                    show("CONNECTED", green, "Listening on 127.0.0.1:34872")
                    if nextJob.id then
                        local operation = nextJob.command.op or "?"
                        local success, result = pcall(execute, nextJob.command)
                        pendingResult = {id = nextJob.id, ok = success}
                        if success then
                            pendingResult.result = result
                            lastAction = operation .. " - OK"
                        else
                            pendingResult.error = tostring(result)
                            lastAction = operation .. " - ERROR"
                        end
                        show("CONNECTED", green, "Processed job " .. tostring(nextJob.id):sub(1, 8))
                    end
                end)
                if not ok then
                    show("OFFLINE", amber, tostring(message):sub(1, 130))
                end
            end
        end
        task.wait(pendingResult and 0.35 or 0.8)
    end
end)
