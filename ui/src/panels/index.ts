// Static panel bundle (plan §4.4): importing this registers every code panel.
import './demo/demoPanel';
import './uart/terminalPanel';
import './uart/tracePanel';
import './uds/consolePanel';
import './eth/ethPanel';
import './psu/gaugePanel';
import { registerCanPanels } from './can';
registerCanPanels();
