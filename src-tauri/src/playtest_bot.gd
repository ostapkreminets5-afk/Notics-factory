extends Node

const OUT_FLAG := "--noctics-out="
const SECONDS_FLAG := "--noctics-seconds="
const MANIFEST := "manifest.json"
const DEFAULT_SECONDS := 40.0
const FIRST_SHOT_AT := 1.5
const START_AT := 2.5
const AFTER_START_SHOT_AT := 4.5
const PLAY_FROM := 5.5
const PAUSE_LEAD := 9.0
const SHOT_EVERY := 4.0
const HOLD_MOVE := 1.1
const TAP := 0.15
const GAP := 0.25
const CLICK_EVERY := 4
const MAX_ACTIONS := 6
const THUMB_WIDTH := 480
const THUMB_QUALITY := 0.72
const PROBE_STRIDE := 8
const BLANK_SPREAD := 0.03
const TIME_STEP := 0.1
const START_WORDS := ["почати", "грати", "нова гра", "старт", "play", "start", "new game", "begin", "continue", "продовжити"]
const AVOID_WORDS := ["вихід", "вийти", "exit", "quit", "налаштування", "settings", "options", "автори", "credits"]
const AVOID_ACTIONS := ["pause", "menu", "quit", "exit", "back", "cancel", "debug", "пауза"]
const MOVE_WORDS := ["right", "left", "up", "down", "move", "walk", "run", "forward", "backward", "вправо", "вліво"]
const ACTION_ORDER := ["right", "jump", "attack", "shoot", "fire", "left", "up", "forward", "down", "dash", "interact", "use", "action"]
const FALLBACK_ACTIONS := ["ui_right", "ui_accept", "ui_left", "ui_up", "ui_down"]

var out_dir := ""
var seconds := DEFAULT_SECONDS
var started_ms := 0
var timeline: Array = []
var cursor := 0
var actions: Array = []
var held: Array = []
var shots: Array = []
var pressed: Array = []
var scenes: Array = []
var busy := false
var finished := false


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	for argument in OS.get_cmdline_user_args():
		if argument.begins_with(OUT_FLAG):
			out_dir = argument.substr(OUT_FLAG.length())
		elif argument.begins_with(SECONDS_FLAG):
			seconds = maxf(argument.substr(SECONDS_FLAG.length()).to_float(), PLAY_FROM + PAUSE_LEAD)
	if out_dir.is_empty():
		set_process(false)
		return
	DirAccess.make_dir_recursive_absolute(out_dir)
	started_ms = Time.get_ticks_msec()
	timeline = _plan()
	_save()


func _process(_delta: float) -> void:
	if finished:
		return
	_note_scene()
	var now := _elapsed()
	for entry in held:
		if entry["until"] <= now:
			_send(entry["action"], false)
	held = held.filter(func(entry): return entry["until"] > now)
	while not busy and cursor < timeline.size() and timeline[cursor]["at"] <= now:
		var step: Dictionary = timeline[cursor]
		cursor += 1
		_run(step)


func _plan() -> Array:
	var plan: Array = [
		{"at": FIRST_SHOT_AT, "do": "shot", "label": "Перший екран"},
		{"at": START_AT, "do": "start"},
		{"at": AFTER_START_SHOT_AT, "do": "shot", "label": "Після старту"},
	]
	var pause_at := seconds - PAUSE_LEAD
	var at := PLAY_FROM
	var slot := 0
	var next_shot := PLAY_FROM + SHOT_EVERY
	while at < pause_at:
		plan.append({"at": at, "do": "play", "slot": slot})
		at += HOLD_MOVE + GAP
		slot += 1
		if slot % CLICK_EVERY == 0:
			plan.append({"at": at, "do": "click"})
		if at >= next_shot and at < pause_at:
			plan.append({"at": at, "do": "shot", "label": "Гра"})
			next_shot += SHOT_EVERY
	plan.append({"at": pause_at, "do": "release"})
	plan.append({"at": pause_at + 0.3, "do": "tap", "action": "ui_cancel"})
	plan.append({"at": pause_at + 1.8, "do": "shot", "label": "Пауза"})
	plan.append({"at": pause_at + 3.0, "do": "tap", "action": "ui_cancel"})
	plan.append({"at": pause_at + 4.8, "do": "shot", "label": "Після паузи"})
	plan.append({"at": seconds, "do": "quit"})
	return plan


func _run(step: Dictionary) -> void:
	match step["do"]:
		"shot":
			_capture(step["label"])
		"start":
			_press_start()
		"play":
			_play(step["slot"])
		"click":
			_click()
		"tap":
			_hold(step["action"], TAP)
		"release":
			_release_all()
		"quit":
			_finish()


func _play(slot: int) -> void:
	if actions.is_empty():
		actions = _game_actions()
	if actions.is_empty():
		return
	var action: String = actions[slot % actions.size()]
	var lowered := action.to_lower()
	var moving := MOVE_WORDS.any(func(word): return lowered.contains(word))
	_hold(action, HOLD_MOVE if moving else TAP)


func _game_actions() -> Array:
	var found: Array = []
	for entry in InputMap.get_actions():
		var action := String(entry)
		var lowered := action.to_lower()
		if action.begins_with("ui_") or action.contains("/"):
			continue
		if AVOID_ACTIONS.any(func(word): return lowered.contains(word)):
			continue
		found.append(action)
	if found.is_empty():
		return FALLBACK_ACTIONS.filter(func(action): return InputMap.has_action(action))
	found.sort_custom(func(first, second): return _rank(first) < _rank(second))
	return found.slice(0, MAX_ACTIONS)


func _rank(action: String) -> int:
	var lowered := action.to_lower()
	for index in ACTION_ORDER.size():
		if lowered.contains(ACTION_ORDER[index]):
			return index
	return ACTION_ORDER.size()


func _hold(action: String, duration: float) -> void:
	if not InputMap.has_action(action):
		return
	_send(action, true)
	held.append({"action": action, "until": _elapsed() + duration})
	pressed.append("%.1f %s" % [_elapsed(), action])


func _send(action: String, down: bool) -> void:
	var event := InputEventAction.new()
	event.action = action
	event.pressed = down
	event.strength = 1.0 if down else 0.0
	Input.parse_input_event(event)


func _release_all() -> void:
	for entry in held:
		_send(entry["action"], false)
	held.clear()


func _press_start() -> void:
	var button := _start_button()
	if button == null:
		_hold("ui_accept", TAP)
		return
	pressed.append("%.1f «%s»" % [_elapsed(), button.text])
	button.pressed.emit()


func _start_button() -> Button:
	var best: Button = null
	var best_rank := START_WORDS.size()
	var queue: Array = [get_tree().root]
	while not queue.is_empty():
		var node: Node = queue.pop_front()
		queue.append_array(node.get_children())
		if not (node is Button):
			continue
		var button := node as Button
		if button.disabled or not button.is_visible_in_tree():
			continue
		var rank := _start_rank(button.text)
		if rank < best_rank:
			best = button
			best_rank = rank
	return best


func _start_rank(text: String) -> int:
	var lowered := text.to_lower()
	if AVOID_WORDS.any(func(word): return lowered.contains(word)):
		return START_WORDS.size()
	for index in START_WORDS.size():
		if lowered.contains(START_WORDS[index]):
			return index
	return START_WORDS.size()


func _click() -> void:
	var center := get_viewport().get_visible_rect().size / 2.0
	for down in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = MOUSE_BUTTON_LEFT
		event.pressed = down
		event.position = center
		event.global_position = center
		Input.parse_input_event(event)
	pressed.append("%.1f клік" % _elapsed())


func _capture(label: String) -> void:
	busy = true
	await RenderingServer.frame_post_draw
	var image := get_viewport().get_texture().get_image()
	if image != null and not image.is_empty():
		var stem := "%02d" % (shots.size() + 1)
		image.save_png(out_dir.path_join(stem + ".png"))
		var thumb: Image = image.duplicate()
		var height := int(round(THUMB_WIDTH * float(image.get_height()) / float(image.get_width())))
		thumb.resize(THUMB_WIDTH, maxi(height, 1), Image.INTERPOLATE_BILINEAR)
		thumb.save_jpg(out_dir.path_join(stem + ".jpg"), THUMB_QUALITY)
		shots.append({
			"file": stem + ".png",
			"thumb": stem + ".jpg",
			"label": label,
			"second": snapped(_elapsed(), TIME_STEP),
			"scene": _scene_name(),
			"blank": _blank(image),
		})
		_save()
	busy = false


func _blank(image: Image) -> bool:
	var darkest := 1.0
	var brightest := 0.0
	for y in range(0, image.get_height(), PROBE_STRIDE):
		for x in range(0, image.get_width(), PROBE_STRIDE):
			var luminance := image.get_pixel(x, y).get_luminance()
			darkest = minf(darkest, luminance)
			brightest = maxf(brightest, luminance)
	return brightest - darkest < BLANK_SPREAD


func _scene_name() -> String:
	var scene := get_tree().current_scene
	return "" if scene == null else String(scene.name)


func _note_scene() -> void:
	var current := _scene_name()
	if not current.is_empty() and (scenes.is_empty() or scenes.back() != current):
		scenes.append(current)


func _save() -> void:
	var file := FileAccess.open(out_dir.path_join(MANIFEST), FileAccess.WRITE)
	if file == null:
		return
	file.store_string(JSON.stringify({
		"shots": shots,
		"pressed": pressed,
		"scenes": scenes,
		"seconds": snapped(_elapsed(), TIME_STEP),
		"finished": finished,
	}))
	file.close()


func _finish() -> void:
	_release_all()
	finished = true
	_save()
	get_tree().quit()


func _elapsed() -> float:
	return (Time.get_ticks_msec() - started_ms) / 1000.0
