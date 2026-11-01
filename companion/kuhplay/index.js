/**
 * KuhPlay – Companion-Modul (Bitfocus Companion)
 *
 * Spricht das TCP-JSON-lines-Protokoll der KuhPlay-App (Default-Port 9999):
 *   Companion → App:  {"action":"go"} / {"action":"go_to","cue":"5.1"} / …
 *   App → Companion:  {"type":"status",…} / {"type":"cue_started",…} / …
 *
 * Eine Nachricht pro Zeile (\n-getrennt), siehe src/remote/companion.rs.
 */

const { InstanceBase, InstanceStatus, runEntrypoint, TCPHelper } = require('@companion-module/base')

class KuhPlayInstance extends InstanceBase {
	constructor(internal) {
		super(internal)
		this.config = {}
		this.socket = undefined
		this.status = null // letzter RemoteStatus-Snapshot der App
		this.running = new Map() // Cue-ID → Cue-Nummer (laufende Cues)
		this.rxBuffer = '' // Zeilen-Puffer für TCP-Daten
	}

	async init(config) {
		this.config = config
		this.setActionDefinitions(this.getActionDefinitions())
		this.setFeedbackDefinitions(this.getFeedbackDefinitions())
		this.setVariableDefinitions(this.getVariableDefinitions())
		this.setVariableValues({ connected: 'no' })
		this.connect()
	}

	async configUpdated(config) {
		this.config = config
		this.connect()
	}

	connect() {
		if (this.socket) {
			this.socket.destroy()
			this.socket = undefined
		}
		if (!this.config.host) {
			this.updateStatus(InstanceStatus.BadConfig, 'Host fehlt in der Konfiguration')
			return
		}
		const port = this.config.port || 9999
		this.rxBuffer = ''
		this.updateStatus(InstanceStatus.Connecting)
		this.socket = new TCPHelper(this.config.host, port)

		this.socket.on('connect', () => {
			this.updateStatus(InstanceStatus.Ok)
			this.setVariableValues({ connected: 'yes' })
			// App pusht den Status ohnehin regelmäßig; explizit anfordern
			// schadet nicht (Antwort: {"type":"status",…}).
			this.sendCommand({ action: 'get_status' })
		})
		this.socket.on('error', (err) => {
			this.updateStatus(InstanceStatus.ConnectionFailure, String(err))
			this.setVariableValues({ connected: 'no' })
			this.log('error', `TCP-Fehler: ${err}`)
		})
		this.socket.on('end', () => {
			this.updateStatus(InstanceStatus.Disconnected)
			this.setVariableValues({ connected: 'no' })
		})
		this.socket.on('data', (data) => this.handleData(data))
	}

	/** Zerlegt den TCP-Stream in JSON-Zeilen und verteilt die Events. */
	handleData(data) {
		this.rxBuffer += data.toString('utf8')
		let idx
		while ((idx = this.rxBuffer.indexOf('\n')) >= 0) {
			const line = this.rxBuffer.slice(0, idx).trim()
			this.rxBuffer = this.rxBuffer.slice(idx + 1)
			if (!line) continue
			let ev
			try {
				ev = JSON.parse(line)
			} catch {
				this.log('warn', `ungültige Zeile von KuhPlay: ${line}`)
				continue
			}
			this.handleEvent(ev)
		}
	}

	handleEvent(ev) {
		switch (ev.type) {
			case 'status':
				this.status = ev
				// Laufende Cues aus dem Snapshot übernehmen.
				this.running = new Map((ev.running || []).map((c) => [c.id, c.number]))
				this.updateVariables()
				this.checkFeedbacks('cue_running')
				break
			case 'cue_started':
				this.running.set(ev.id, ev.number)
				this.checkFeedbacks('cue_running')
				break
			case 'cue_finished':
			case 'cue_stopped':
				this.running.delete(ev.id)
				this.checkFeedbacks('cue_running')
				break
			case 'cue_error':
				this.running.delete(ev.id)
				this.log('warn', `Cue ${ev.id} Fehler: ${ev.message}`)
				this.checkFeedbacks('cue_running')
				break
			case 'panicked':
				this.running.clear()
				this.checkFeedbacks('cue_running')
				break
			default:
				// cue_paused / cue_resumed / playhead_moved / standby_changed /
				// show_mode_changed / pong – Variablen aktualisieren sich
				// über den nächsten Status-Snapshot (1×/s von der App).
				break
		}
	}

	/** Sendet ein RemoteCommand als JSON-Zeile an die App. */
	sendCommand(obj) {
		if (this.socket && this.socket.isConnected) {
			this.socket.send(JSON.stringify(obj) + '\n')
		}
	}

	updateVariables() {
		const s = this.status
		if (!s) return
		this.setVariableValues({
			show_name: s.show_name || '',
			show_mode: s.show_mode ? 'An' : 'Aus',
			active_cart_name: s.active_cart_name || '',
			playhead: String(s.playhead ?? 0),
			standby_number: s.standby ? s.standby.number : '',
			standby_name: s.standby ? s.standby.name : '',
			running_count: String((s.running || []).length),
		})
	}

	async destroy() {
		if (this.socket) {
			this.socket.destroy()
			this.socket = undefined
		}
	}

	getConfigFields() {
		return [
			{
				type: 'textinput',
				id: 'host',
				label: 'Ziel-Host (KuhPlay-Rechner)',
				width: 6,
				default: '127.0.0.1',
			},
			{
				type: 'number',
				id: 'port',
				label: 'TCP-Port',
				width: 3,
				min: 1,
				max: 65535,
				default: 9999,
			},
		]
	}

	getActionDefinitions() {
		return {
			go: {
				name: 'GO (nächster Cue)',
				options: [],
				callback: () => this.sendCommand({ action: 'go' }),
			},
			panic: {
				name: 'Panic (alles stoppen)',
				options: [],
				callback: () => this.sendCommand({ action: 'panic' }),
			},
			pause_all: {
				name: 'Pause All',
				options: [],
				callback: () => this.sendCommand({ action: 'pause_all' }),
			},
			resume_all: {
				name: 'Resume All',
				options: [],
				callback: () => this.sendCommand({ action: 'resume_all' }),
			},
			reset: {
				name: 'Reset (alles stoppen + Playhead zurück)',
				options: [],
				callback: () => this.sendCommand({ action: 'reset' }),
			},
			go_to: {
				name: 'Cue starten (GoTo)',
				options: [
					{
						type: 'textinput',
						id: 'cue',
						label: 'Cue-Nummer (z. B. 5.1)',
						default: '1',
					},
				],
				callback: (action) =>
					this.sendCommand({ action: 'go_to', cue: action.options.cue }),
			},
			arm: {
				name: 'Cue arm/disarm',
				options: [
					{
						type: 'textinput',
						id: 'cue',
						label: 'Cue-Nummer (z. B. 5.1)',
						default: '1',
					},
					{
						type: 'checkbox',
						id: 'armed',
						label: 'Armed',
						default: true,
					},
				],
				callback: (action) =>
					this.sendCommand({
						action: 'arm',
						cue: action.options.cue,
						armed: !!action.options.armed,
					}),
			},
			set_show_mode: {
				name: 'Show-Mode setzen',
				options: [
					{
						type: 'checkbox',
						id: 'enabled',
						label: 'Show-Mode aktiv',
						default: true,
					},
				],
				callback: (action) =>
					this.sendCommand({
						action: 'set_show_mode',
						enabled: !!action.options.enabled,
					}),
			},
			get_status: {
				name: 'Status anfordern',
				options: [],
				callback: () => this.sendCommand({ action: 'get_status' }),
			},
		}
	}

	getVariableDefinitions() {
		return [
			{ variableId: 'connected', name: 'Verbunden (yes/no)' },
			{ variableId: 'show_name', name: 'Show-Name' },
			{ variableId: 'show_mode', name: 'Show-Mode (An/Aus)' },
			{ variableId: 'active_cart_name', name: 'Aktiver Cart' },
			{ variableId: 'playhead', name: 'Playhead (Index)' },
			{ variableId: 'standby_number', name: 'Standby-Cue (Nummer)' },
			{ variableId: 'standby_name', name: 'Standby-Cue (Name)' },
			{ variableId: 'running_count', name: 'Anzahl laufender Cues' },
		]
	}

	getFeedbackDefinitions() {
		return {
			cue_running: {
				type: 'boolean',
				name: 'Cue läuft',
				description: 'Ist aktiv, während der Cue mit dieser Nummer läuft.',
				options: [
					{
						type: 'textinput',
						id: 'cue',
						label: 'Cue-Nummer (z. B. 5.1)',
						default: '1',
					},
				],
				defaultStyle: {
					bgcolor: 0x2ecc71,
					color: 0x000000,
				},
				callback: (feedback) => this.runningNumbers().has(feedback.options.cue),
			},
		}
	}

	/** Nummern aller aktuell laufenden Cues. */
	runningNumbers() {
		return new Set(this.running.values())
	}
}

runEntrypoint(KuhPlayInstance, [])
