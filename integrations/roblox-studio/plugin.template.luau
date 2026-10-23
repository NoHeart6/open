-- ChatGPT Studio Bridge: local editor commands over 127.0.0.1
assert(plugin, "Install this as a local Roblox Studio plugin")

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local History = game:GetService("ChangeHistoryService")
local ScriptEditor = game:GetService("ScriptEditorService")
local Tags = game:GetService("CollectionService")
local InsertService = game:GetService("InsertService")
local LogService = game:GetService("LogService")
local RunService = game:GetService("RunService")
local BRIDGE_VERSION = "3.0.1"
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

local function handle(command, insideBatch)
    if type(command) ~= "table" or type(command.op) ~= "string" then error("Missing op") end
    local op = command.op
    if op == "ping" then
        return {pong = true, place = game.Name, placeId = game.PlaceId, client = CLIENT, version = BRIDGE_VERSION}
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
                "insert_asset", "batch", "undo", "redo", "run_state", "logs", "diagnostics", "logs_clear", "test_run", "test_pause", "test_resume", "test_stop"},
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
