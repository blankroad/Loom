import { InterfaceConnect } from './InterfaceConnect';

/** Covers only the unconfigured default workspace; the dock stays attached beneath it. */
export function WorkbenchWelcome() {
  return (
    <section className="workbench-welcome" aria-labelledby="welcome-title" data-testid="workbench-welcome">
      <div className="welcome-content">
        <span className="welcome-symbol" aria-hidden="true">⌘</span>
        <span className="eyebrow">YOUR DEVICE WORKSPACE</span>
        <h2 id="welcome-title">연결에서 관찰까지, 한곳에서</h2>
        <p className="welcome-intro">현재 연결된 인터페이스가 없습니다. 장치나 시뮬레이터를 추가하고, 필요한 보기를 나란히 열어 작업을 시작하세요.</p>
        <InterfaceConnect />
        <ol className="welcome-steps">
          <li><span className="welcome-step-number">01</span><div><h3>인터페이스 추가</h3><p>UART, CAN, UDS, Ethernet 연결을 설정하세요. 실제 장치 없이 시뮬레이터로 시작할 수도 있습니다.</p></div></li>
          <li><span className="welcome-step-number">02</span><div><h3>채널과 보기 선택</h3><p>왼쪽 탐색기에서 채널을 찾아 터미널·트레이스 등 필요한 보기를 여세요. 보기 선택만으로 송신하지 않습니다.</p></div></li>
          <li><span className="welcome-step-number">03</span><div><h3>작업 공간 구성</h3><p>패널 탭을 끌어 나란히 배치하고 레이아웃을 저장하세요. 하단 Activity에서 작업 기록을 확인할 수 있습니다.</p></div></li>
        </ol>
        <p className="welcome-safety"><span aria-hidden="true">■</span> 상단 STOP은 agent·script 쓰기와 잡을 잠급니다. 사용자 입력은 계속 가능합니다.</p>
      </div>
    </section>
  );
}
