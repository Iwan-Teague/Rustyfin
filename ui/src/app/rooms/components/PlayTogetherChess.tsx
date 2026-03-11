'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import {
  WsBattleshipState,
  WsConnectFourState,
  WsPlayStateMessage,
  WsPresenceMember,
} from '@/lib/watchPartyApi';

type Props = {
  playState: WsPlayStateMessage | null;
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

type PlayGameKey = 'chess' | 'connect_four' | 'battleship';

const FILES = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'] as const;
const RANKS = ['8', '7', '6', '5', '4', '3', '2', '1'] as const;
const CONNECT_FOUR_COLUMNS = 7;
const CONNECT_FOUR_ROWS = 6;
const BATTLESHIP_SIZE = 10;

const PIECE_SYMBOLS: Record<string, string> = {
  P: '♟',
  N: '♞',
  B: '♝',
  R: '♜',
  Q: '♛',
  K: '♚',
  p: '♙',
  n: '♘',
  b: '♗',
  r: '♖',
  q: '♕',
  k: '♔',
};

const PROMOTION_OPTIONS_WHITE = [
  { label: 'Queen', piece: 'Q', promotion: 'q' },
  { label: 'Rook', piece: 'R', promotion: 'r' },
  { label: 'Bishop', piece: 'B', promotion: 'b' },
  { label: 'Knight', piece: 'N', promotion: 'n' },
] as const;

const PROMOTION_OPTIONS_BLACK = [
  { label: 'Queen', piece: 'q', promotion: 'q' },
  { label: 'Rook', piece: 'r', promotion: 'r' },
  { label: 'Bishop', piece: 'b', promotion: 'b' },
  { label: 'Knight', piece: 'n', promotion: 'n' },
] as const;

const AI_DIFFICULTY_OPTIONS: Array<{ value: AiDifficulty; label: string }> = [
  { value: 'easy', label: 'Easy' },
  { value: 'medium', label: 'Medium' },
  { value: 'hard', label: 'Hard' },
];

const BATTLESHIP_FLEET = [
  { id: 1, name: 'Carrier', size: 5 },
  { id: 2, name: 'Battleship', size: 4 },
  { id: 3, name: 'Cruiser', size: 3 },
  { id: 4, name: 'Submarine', size: 3 },
  { id: 5, name: 'Destroyer', size: 2 },
] as const;

function parseFenBoard(fen: string): Map<string, string> {
  const board = new Map<string, string>();
  const placement = fen.split(' ')[0] || '';
  const rows = placement.split('/');
  if (rows.length !== 8) return board;

  for (let rankIndex = 0; rankIndex < 8; rankIndex += 1) {
    const row = rows[rankIndex] || '';
    let fileIndex = 0;
    for (const token of row) {
      if (/\d/.test(token)) {
        fileIndex += Number(token);
        continue;
      }
      if (fileIndex > 7) continue;
      const square = `${FILES[fileIndex]}${8 - rankIndex}`;
      board.set(square, token);
      fileIndex += 1;
    }
  }

  return board;
}

function displayChessStatus(status: string): string {
  if (status === 'checkmate') return 'Checkmate';
  if (status === 'stalemate') return 'Stalemate';
  return 'Active';
}

function displayConnectFourStatus(status: string): string {
  if (status === 'win') return 'Win';
  if (status === 'draw') return 'Draw';
  return 'Active';
}

function displayBattleshipPhase(phase: string): string {
  if (phase === 'active') return 'Active';
  if (phase === 'finished') return 'Finished';
  return 'Setup';
}

function nameForUser(userId: string | null | undefined, members: WsPresenceMember[]): string {
  if (!userId) return 'Unassigned';
  return members.find((member) => member.user_id === userId)?.username ?? userId;
}

function pieceColor(piece: string): 'white' | 'black' {
  return piece === piece.toUpperCase() ? 'white' : 'black';
}

function randomSeatAssignment(
  members: WsPresenceMember[],
): { firstUserId: string; secondUserId: string } | null {
  const connected = members.filter((member) => member.connected !== false);
  if (connected.length < 2) return null;
  const ids = connected.map((member) => member.user_id);
  const firstIndex = Math.floor(Math.random() * ids.length);
  const firstUserId = ids.splice(firstIndex, 1)[0];
  const secondIndex = Math.floor(Math.random() * ids.length);
  const secondUserId = ids[secondIndex];
  if (!firstUserId || !secondUserId || firstUserId === secondUserId) {
    return null;
  }
  return { firstUserId, secondUserId };
}

function normalizeConnectFourRows(rows: string[] | undefined): string[] {
  if (!rows || rows.length !== CONNECT_FOUR_ROWS) {
    return Array.from({ length: CONNECT_FOUR_ROWS }, () => '.'.repeat(CONNECT_FOUR_COLUMNS));
  }
  return rows.map((row) => {
    if (row.length !== CONNECT_FOUR_COLUMNS) {
      return '.'.repeat(CONNECT_FOUR_COLUMNS);
    }
    return row;
  });
}

function normalizeBattleshipRows(rows: string[] | undefined): string[][] {
  if (!rows || rows.length !== BATTLESHIP_SIZE) {
    return Array.from({ length: BATTLESHIP_SIZE }, () => Array.from({ length: BATTLESHIP_SIZE }, () => '.'));
  }
  return rows.map((row) => {
    const trimmed = row.length === BATTLESHIP_SIZE ? row : '.'.repeat(BATTLESHIP_SIZE);
    return trimmed.split('');
  });
}

type PendingPromotion = {
  from: string;
  to: string;
  forColor: 'white' | 'black';
};

type SidePanelTab = 'local' | 'ai';
type AiDifficulty = 'easy' | 'medium' | 'hard';
type HumanColorPreference = 'white' | 'black' | 'random';
type BattleshipOrientation = 'horizontal' | 'vertical';

type ChessPanelProps = {
  chess: WsPlayStateMessage['chess'];
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

function ChessGamePanel({ chess, members, currentUserId, canControl, sendWs }: ChessPanelProps) {
  const [selectedSquare, setSelectedSquare] = useState<string | null>(null);
  const [pendingPromotion, setPendingPromotion] = useState<PendingPromotion | null>(null);
  const [sidePanelTab, setSidePanelTab] = useState<SidePanelTab>('local');
  const [aiDifficulty, setAiDifficulty] = useState<AiDifficulty>('medium');
  const [humanColorPreference, setHumanColorPreference] = useState<HumanColorPreference>('white');
  const autoAssignedRef = useRef(false);

  const board = useMemo(() => parseFenBoard(chess?.fen ?? ''), [chess?.fen]);
  const whiteUserId = chess?.white_user_id ?? null;
  const blackUserId = chess?.black_user_id ?? null;
  const aiEnabled = chess?.ai_enabled === true;
  const aiDifficultyValue: AiDifficulty =
    chess?.ai_difficulty === 'easy' || chess?.ai_difficulty === 'hard'
      ? chess.ai_difficulty
      : 'medium';
  const aiColor =
    chess?.ai_color === 'white' || chess?.ai_color === 'black' ? chess.ai_color : null;
  const humanColorActive: 'white' | 'black' | null = aiColor
    ? aiColor === 'white'
      ? 'black'
      : 'white'
    : null;
  const turn = chess?.turn === 'black' ? 'black' : 'white';
  const myAssignedColor: 'white' | 'black' | null =
    whiteUserId === currentUserId ? 'white' : blackUserId === currentUserId ? 'black' : null;
  const hasWhitePlayer = !!whiteUserId;
  const hasBlackPlayer = !!blackUserId;
  const hasAnyAssignedPlayer = hasWhitePlayer || hasBlackPlayer;
  const requiresDualResetConfirm = hasWhitePlayer && hasBlackPlayer && whiteUserId !== blackUserId;
  const canRequestBoardReset = !hasAnyAssignedPlayer || !!myAssignedColor;
  const resetRequestedWhite = chess?.reset_requested_white === true;
  const resetRequestedBlack = chess?.reset_requested_black === true;
  const myResetRequested =
    myAssignedColor === 'white'
      ? resetRequestedWhite
      : myAssignedColor === 'black'
        ? resetRequestedBlack
        : false;
  const turnUserId = turn === 'white' ? whiteUserId : blackUserId;
  const turnOwnerName = nameForUser(turnUserId, members);
  const isMyTurnSeatAssigned = !!turnUserId && turnUserId === currentUserId;
  const canMove = canControl && isMyTurnSeatAssigned && chess?.status === 'active';
  const filesForDisplay = myAssignedColor === 'black' ? [...FILES].reverse() : [...FILES];
  const ranksForDisplay = myAssignedColor === 'black' ? [...RANKS].reverse() : [...RANKS];

  const selectedPiece = selectedSquare ? board.get(selectedSquare) : undefined;
  const selectedLegalTargets = useMemo(() => {
    if (!selectedSquare) return new Set<string>();
    const moves = chess?.legal_moves ?? [];
    const destinations = new Set<string>();
    for (const move of moves) {
      if (move.from === selectedSquare) {
        destinations.add(move.to);
      }
    }
    return destinations;
  }, [chess?.legal_moves, selectedSquare]);

  useEffect(() => {
    if (whiteUserId || blackUserId) {
      autoAssignedRef.current = false;
    }
  }, [whiteUserId, blackUserId]);

  useEffect(() => {
    if (!chess || !canControl) return;
    if (whiteUserId || blackUserId) return;
    if (autoAssignedRef.current) return;
    const assignment = randomSeatAssignment(members);
    if (!assignment) return;
    autoAssignedRef.current = true;
    sendWs({
      type: 'chess_set_players',
      white_user_id: assignment.firstUserId,
      black_user_id: assignment.secondUserId,
    });
  }, [chess, canControl, whiteUserId, blackUserId, members, sendWs]);

  useEffect(() => {
    setSelectedSquare(null);
    setPendingPromotion(null);
  }, [chess?.fen]);

  useEffect(() => {
    if (aiEnabled) {
      setSidePanelTab('ai');
    }
  }, [aiEnabled]);

  useEffect(() => {
    setAiDifficulty(aiDifficultyValue);
    if (humanColorActive) {
      setHumanColorPreference(humanColorActive);
    }
  }, [aiDifficultyValue, humanColorActive]);

  function handleAssignPlayers(nextWhite: string | null, nextBlack: string | null) {
    if (!canControl) return;
    sendWs({
      type: 'chess_set_players',
      white_user_id: nextWhite || null,
      black_user_id: nextBlack || null,
    });
  }

  function sendAiConfig(
    enabled: boolean,
    nextDifficulty: AiDifficulty = aiDifficulty,
    nextHumanColorPreference: HumanColorPreference = humanColorPreference,
  ) {
    if (!canControl) return;
    const resolvedHumanColor: 'white' | 'black' =
      nextHumanColorPreference === 'random'
        ? Math.random() < 0.5
          ? 'white'
          : 'black'
        : nextHumanColorPreference;
    sendWs({
      type: 'chess_configure_ai',
      enabled,
      difficulty: nextDifficulty,
      human_color: enabled ? resolvedHumanColor : undefined,
    });
  }

  function handleSelectSidePanelTab(nextTab: SidePanelTab) {
    setSidePanelTab(nextTab);
    if (nextTab === 'ai' && !aiEnabled) {
      sendAiConfig(true);
      return;
    }
    if (nextTab === 'local' && aiEnabled) {
      sendAiConfig(false);
    }
  }

  function handleSquareClick(square: string) {
    if (!chess || !canMove) return;
    const clickedPiece = board.get(square);

    if (!selectedSquare) {
      if (!clickedPiece) return;
      if (!myAssignedColor || pieceColor(clickedPiece) !== myAssignedColor) {
        return;
      }
      setSelectedSquare(square);
      return;
    }

    if (selectedSquare === square) {
      setSelectedSquare(null);
      return;
    }

    if (clickedPiece && myAssignedColor && pieceColor(clickedPiece) === myAssignedColor) {
      setSelectedSquare(square);
      return;
    }

    const isPawnPromotion =
      selectedPiece &&
      (selectedPiece === 'P' || selectedPiece === 'p') &&
      ((selectedPiece === 'P' && square.endsWith('8')) ||
        (selectedPiece === 'p' && square.endsWith('1')));

    if (isPawnPromotion && myAssignedColor) {
      setPendingPromotion({
        from: selectedSquare,
        to: square,
        forColor: myAssignedColor,
      });
      return;
    }

    sendWs({
      type: 'chess_move',
      from: selectedSquare,
      to: square,
      promotion: null,
    });
    setSelectedSquare(null);
  }

  function applyPromotion(promotion: 'q' | 'r' | 'b' | 'n') {
    if (!pendingPromotion) return;
    sendWs({
      type: 'chess_move',
      from: pendingPromotion.from,
      to: pendingPromotion.to,
      promotion,
    });
    setPendingPromotion(null);
    setSelectedSquare(null);
  }

  function handleStartNewGame(randomizeSeats: boolean) {
    if (!canRequestBoardReset) return;
    if (requiresDualResetConfirm && myResetRequested) return;
    sendWs({ type: 'chess_reset' });
    if (!randomizeSeats || aiEnabled || !canControl) {
      return;
    }
    const assignment = randomSeatAssignment(members);
    if (!assignment) {
      return;
    }
    sendWs({
      type: 'chess_set_players',
      white_user_id: assignment.firstUserId,
      black_user_id: assignment.secondUserId,
    });
  }

  const isGameOver = chess.status === 'checkmate' || chess.status === 'stalemate';
  const winnerColor = chess.winner_color === 'white' || chess.winner_color === 'black' ? chess.winner_color : null;
  const gameOverTitle = chess.status === 'stalemate' ? 'Stalemate' : `${winnerColor === 'white' ? 'White' : 'Black'} won`;
  const gameOverSummary =
    chess.status === 'stalemate'
      ? 'The game ended in a draw by stalemate.'
      : myAssignedColor
        ? myAssignedColor === winnerColor
          ? 'You won this game.'
          : 'You lost this game.'
        : 'Game over.';

  return (
    <>
      <div className="relative grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
        <div className="panel-soft rounded-xl p-3 sm:p-4">
          <div className="grid grid-cols-8 overflow-hidden rounded-lg border border-white/10">
            {ranksForDisplay.flatMap((rank, rankIndex) =>
              filesForDisplay.map((file, fileIndex) => {
                const square = `${file}${rank}`;
                const piece = board.get(square);
                const isLight = (rankIndex + fileIndex) % 2 !== 0;
                const isSelected = selectedSquare === square;
                const isLastMove = chess.last_move_from === square || chess.last_move_to === square;
                const isLegalDestination = !!selectedSquare && selectedLegalTargets.has(square);
                return (
                  <button
                    key={square}
                    type="button"
                    onClick={() => handleSquareClick(square)}
                    className={`relative flex h-12 items-center justify-center border border-white/5 text-2xl sm:h-14 ${
                      isLight ? 'bg-white/5' : 'bg-black/20'
                    } ${isSelected ? 'ring-2 ring-[var(--orange-soft)] ring-inset' : ''} ${
                      isLastMove ? 'outline outline-1 outline-[var(--purple)] outline-offset-[-1px]' : ''
                    }`}
                  >
                    {isLegalDestination &&
                      (piece ? (
                        <span className="pointer-events-none absolute inset-[18%] rounded-full border-2 border-[var(--orange-soft)]/85" />
                      ) : (
                        <span className="pointer-events-none absolute h-3 w-3 rounded-full bg-[var(--orange-soft)]/80" />
                      ))}
                    <span className={piece && piece === piece.toLowerCase() ? 'text-white/90' : 'text-white'}>
                      {piece ? PIECE_SYMBOLS[piece] : ''}
                    </span>
                    <span className="pointer-events-none absolute bottom-0.5 right-1 text-[9px] text-white/35">{square}</span>
                  </button>
                );
              }),
            )}
          </div>
        </div>

        <aside className="panel-soft space-y-3 rounded-xl p-3 sm:p-4">
          <div className="space-y-2">
            <div className="flex gap-2">
              <button
                type="button"
                className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                  sidePanelTab === 'local' ? 'btn-primary' : 'btn-secondary'
                }`}
                onClick={() => handleSelectSidePanelTab('local')}
              >
                Local
              </button>
              <button
                type="button"
                className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                  sidePanelTab === 'ai' ? 'btn-primary' : 'btn-secondary'
                }`}
                onClick={() => handleSelectSidePanelTab('ai')}
              >
                AI
              </button>
            </div>
          </div>

          {sidePanelTab === 'local' ? (
            <>
              <div className="space-y-1">
                <p className="text-xs uppercase tracking-wide muted">Players</p>
                <p className="text-xs muted">Assign white and black seats, or leave unassigned for open turns.</p>
              </div>

              <label className="block text-sm">
                <span className="mb-1 block text-xs uppercase tracking-wide muted">White</span>
                <select
                  className="select px-2 py-2 text-sm"
                  value={whiteUserId ?? ''}
                  onChange={(event) => handleAssignPlayers(event.target.value || null, blackUserId)}
                  disabled={!canControl}
                >
                  <option value="">Unassigned</option>
                  {members.map((member) => (
                    <option key={member.user_id} value={member.user_id}>
                      {member.username}
                    </option>
                  ))}
                </select>
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-xs uppercase tracking-wide muted">Black</span>
                <select
                  className="select px-2 py-2 text-sm"
                  value={blackUserId ?? ''}
                  onChange={(event) => handleAssignPlayers(whiteUserId, event.target.value || null)}
                  disabled={!canControl}
                >
                  <option value="">Unassigned</option>
                  {members.map((member) => (
                    <option key={member.user_id} value={member.user_id}>
                      {member.username}
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : (
            <>
              <div className="space-y-1">
                <p className="text-xs uppercase tracking-wide muted">AI Opponent</p>
                <p className="text-xs muted">Play against server AI. Choose a difficulty and your side.</p>
              </div>

              <label className="block text-sm">
                <span className="mb-1 block text-xs uppercase tracking-wide muted">Difficulty</span>
                <select
                  className="select px-2 py-2 text-sm"
                  value={aiDifficulty}
                  onChange={(event) => {
                    const nextDifficulty = event.target.value as AiDifficulty;
                    setAiDifficulty(nextDifficulty);
                    if (sidePanelTab === 'ai') {
                      sendAiConfig(true, nextDifficulty, humanColorPreference);
                    }
                  }}
                  disabled={!canControl}
                >
                  {AI_DIFFICULTY_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-xs uppercase tracking-wide muted">You Play As</span>
                <select
                  className="select px-2 py-2 text-sm"
                  value={humanColorPreference}
                  onChange={(event) => {
                    const nextPreference = event.target.value as HumanColorPreference;
                    setHumanColorPreference(nextPreference);
                    if (sidePanelTab === 'ai') {
                      sendAiConfig(true, aiDifficulty, nextPreference);
                    }
                  }}
                  disabled={!canControl}
                >
                  <option value="white">White</option>
                  <option value="black">Black</option>
                  <option value="random">Random</option>
                </select>
              </label>
            </>
          )}

          <div className="space-y-1 text-xs muted">
            <p>White: {nameForUser(whiteUserId, members)}</p>
            <p>Black: {nameForUser(blackUserId, members)}</p>
            <p>Turn owner: {turnOwnerName}</p>
          </div>

          <button
            type="button"
            className="btn-secondary w-full px-3 py-2 text-sm"
            onClick={() => sendWs({ type: 'chess_reset' })}
            disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
          >
            {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'Reset Board'}
          </button>
        </aside>

        {isGameOver && (
          <div className="absolute inset-0 z-20 flex items-center justify-center rounded-xl bg-black/55 p-4 backdrop-blur-[2px]">
            <div className="panel w-full max-w-md space-y-3 rounded-2xl border border-[var(--border)] p-5">
              <h3 className="text-xl font-semibold">{gameOverTitle}</h3>
              <p className="text-sm muted">{gameOverSummary}</p>
              <div className="flex flex-wrap justify-end gap-2 pt-1">
                <button
                  type="button"
                  className="btn-primary px-4 py-2 text-sm"
                  onClick={() => handleStartNewGame(false)}
                  disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
                >
                  {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'New Game'}
                </button>
                {!aiEnabled && (
                  <button
                    type="button"
                    className="btn-secondary px-4 py-2 text-sm"
                    onClick={() => handleStartNewGame(true)}
                    disabled={!canControl || !canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
                  >
                    New Game (Random Colors)
                  </button>
                )}
              </div>
              {!canRequestBoardReset && (
                <p className="text-xs muted">Only active players can reset while seats are occupied.</p>
              )}
            </div>
          </div>
        )}
      </div>

      {pendingPromotion && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]">
          <div className="panel w-full max-w-sm space-y-4 rounded-2xl border border-[var(--border)] p-5">
            <h3 className="text-lg font-semibold">Choose Promotion Piece</h3>
            <p className="text-sm muted">Pick which piece this pawn should become.</p>
            <div className="grid grid-cols-2 gap-2">
              {(pendingPromotion.forColor === 'white' ? PROMOTION_OPTIONS_WHITE : PROMOTION_OPTIONS_BLACK).map((option) => (
                <button
                  key={option.promotion}
                  type="button"
                  className="btn-secondary flex items-center justify-center gap-2 px-3 py-2 text-sm"
                  onClick={() => applyPromotion(option.promotion)}
                >
                  <span className="text-xl">{PIECE_SYMBOLS[option.piece]}</span>
                  <span>{option.label}</span>
                </button>
              ))}
            </div>
            <div className="flex justify-end">
              <button type="button" className="btn-ghost px-3 py-2 text-sm" onClick={() => setPendingPromotion(null)}>
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

type ConnectFourPanelProps = {
  connectFour: WsConnectFourState;
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

type ConnectFourHumanColorPreference = 'red' | 'blue' | 'random';

function ConnectFourGamePanel({ connectFour, members, currentUserId, canControl, sendWs }: ConnectFourPanelProps) {
  const rows = useMemo(() => normalizeConnectFourRows(connectFour.board_rows), [connectFour.board_rows]);
  const [dropPending, setDropPending] = useState(false);
  const [sidePanelTab, setSidePanelTab] = useState<SidePanelTab>('local');
  const [aiDifficulty, setAiDifficulty] = useState<AiDifficulty>('medium');
  const [humanColorPreference, setHumanColorPreference] =
    useState<ConnectFourHumanColorPreference>('red');
  const redUserId = connectFour.red_user_id ?? null;
  const yellowUserId = connectFour.yellow_user_id ?? null;
  const aiEnabled = connectFour.ai_enabled === true;
  const aiDifficultyValue: AiDifficulty =
    connectFour.ai_difficulty === 'easy' || connectFour.ai_difficulty === 'hard'
      ? connectFour.ai_difficulty
      : 'medium';
  const aiColor =
    connectFour.ai_color === 'red' || connectFour.ai_color === 'yellow'
      ? connectFour.ai_color
      : null;
  const humanColorActive: 'red' | 'blue' | null = aiColor
    ? aiColor === 'red'
      ? 'blue'
      : 'red'
    : null;
  const myColor: 'red' | 'yellow' | null =
    redUserId === currentUserId ? 'red' : yellowUserId === currentUserId ? 'yellow' : null;
  const turn = connectFour.turn === 'yellow' ? 'yellow' : 'red';
  const turnUserId = turn === 'red' ? redUserId : yellowUserId;
  const status = connectFour.status;
  const winnerColor = connectFour.winner_color === 'yellow' ? 'yellow' : connectFour.winner_color === 'red' ? 'red' : null;
  const aiStatusLabel = aiEnabled
    ? `AI (${aiDifficultyValue[0].toUpperCase()}${aiDifficultyValue.slice(1)})`
    : null;
  const redSeatName = aiColor === 'red' ? aiStatusLabel ?? 'AI' : nameForUser(redUserId, members);
  const blueSeatName = aiColor === 'yellow' ? aiStatusLabel ?? 'AI' : nameForUser(yellowUserId, members);
  const turnOwnerName = turn === 'red' ? redSeatName : blueSeatName;

  const hasRedPlayer = !!redUserId;
  const hasYellowPlayer = !!yellowUserId;
  const hasAnyAssignedPlayer = hasRedPlayer || hasYellowPlayer;
  const requiresDualResetConfirm = hasRedPlayer && hasYellowPlayer && redUserId !== yellowUserId;
  const canRequestBoardReset = !hasAnyAssignedPlayer || !!myColor;
  const myResetRequested = myColor === 'red' ? connectFour.reset_requested_red : myColor === 'yellow' ? connectFour.reset_requested_yellow : false;

  const canDrop =
    canControl &&
    status === 'active' &&
    !dropPending &&
    !!myColor &&
    turnUserId === currentUserId &&
    myColor === turn;

  useEffect(() => {
    setDropPending(false);
  }, [
    connectFour.updated_ts_ms,
    connectFour.turn,
    connectFour.status,
    connectFour.last_move_col,
    connectFour.last_move_row,
  ]);

  useEffect(() => {
    if (aiEnabled) {
      setSidePanelTab('ai');
    }
  }, [aiEnabled]);

  useEffect(() => {
    setAiDifficulty(aiDifficultyValue);
    if (humanColorActive) {
      setHumanColorPreference(humanColorActive);
    }
  }, [aiDifficultyValue, humanColorActive]);

  function handleAssignPlayers(nextRed: string | null, nextYellow: string | null) {
    if (!canControl) return;
    sendWs({
      type: 'connect_four_set_players',
      red_user_id: nextRed || null,
      yellow_user_id: nextYellow || null,
    });
  }

  function handleDrop(column: number) {
    if (!canDrop) return;
    const sent = sendWs({ type: 'connect_four_drop', column });
    if (sent) {
      setDropPending(true);
    }
  }

  function handleRandomSeats() {
    if (!canControl) return;
    const assignment = randomSeatAssignment(members);
    if (!assignment) return;
    handleAssignPlayers(assignment.firstUserId, assignment.secondUserId);
  }

  function sendAiConfig(
    enabled: boolean,
    nextDifficulty: AiDifficulty = aiDifficulty,
    nextHumanColorPreference: ConnectFourHumanColorPreference = humanColorPreference,
  ) {
    if (!canControl) return;
    const resolvedHumanColor: 'red' | 'blue' =
      nextHumanColorPreference === 'random'
        ? Math.random() < 0.5
          ? 'red'
          : 'blue'
        : nextHumanColorPreference;
    sendWs({
      type: 'connect_four_configure_ai',
      enabled,
      difficulty: nextDifficulty,
      human_color: enabled ? resolvedHumanColor : undefined,
    });
  }

  function handleSelectSidePanelTab(nextTab: SidePanelTab) {
    setSidePanelTab(nextTab);
    if (nextTab === 'ai' && !aiEnabled) {
      sendAiConfig(true);
      return;
    }
    if (nextTab === 'local' && aiEnabled) {
      sendAiConfig(false);
    }
  }

  const isGameOver = status === 'win' || status === 'draw';

  return (
    <div className="relative grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
      <div className="panel-soft rounded-xl p-3 sm:p-4">
        <div className="mb-2 grid grid-cols-7 gap-1">
          {Array.from({ length: CONNECT_FOUR_COLUMNS }, (_, col) => (
            <button
              key={`drop-${col}`}
              type="button"
              className="btn-secondary px-1 py-1.5 text-xs disabled:opacity-40"
              onClick={() => handleDrop(col)}
              disabled={!canDrop || status !== 'active'}
            >
              Drop
            </button>
          ))}
        </div>

        <div className="grid grid-cols-7 gap-1 rounded-xl border border-white/10 bg-black/20 p-2">
          {rows.flatMap((row, rowIndex) =>
            row.split('').map((token, colIndex) => {
              const isRed = token === 'r';
              const isYellow = token === 'y';
              const isLastMove = connectFour.last_move_row === rowIndex && connectFour.last_move_col === colIndex;
              return (
                <div
                  key={`c4-${rowIndex}-${colIndex}`}
                  className={`flex aspect-square items-center justify-center rounded-md bg-black/35 ${
                    isLastMove ? 'ring-2 ring-[var(--orange-soft)] ring-inset' : ''
                  }`}
                >
                  <span
                    className={`h-[70%] w-[70%] rounded-full border border-white/20 ${
                      isRed
                        ? 'bg-red-500'
                        : isYellow
                          ? 'bg-sky-500'
                          : 'bg-black/35'
                    }`}
                  />
                </div>
              );
            }),
          )}
        </div>
      </div>

      <aside className="panel-soft space-y-3 rounded-xl p-3 sm:p-4">
        <div className="space-y-2">
          <div className="flex gap-2">
            <button
              type="button"
              className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                sidePanelTab === 'local' ? 'btn-primary' : 'btn-secondary'
              }`}
              onClick={() => handleSelectSidePanelTab('local')}
            >
              Local
            </button>
            <button
              type="button"
              className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                sidePanelTab === 'ai' ? 'btn-primary' : 'btn-secondary'
              }`}
              onClick={() => handleSelectSidePanelTab('ai')}
            >
              AI
            </button>
          </div>
        </div>

        {sidePanelTab === 'local' ? (
          <>
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide muted">Players</p>
              <p className="text-xs muted">Assign Red/Blue seats to room members.</p>
            </div>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Red</span>
              <select
                className="select px-2 py-2 text-sm"
                value={redUserId ?? ''}
                onChange={(event) => handleAssignPlayers(event.target.value || null, yellowUserId)}
                disabled={!canControl}
              >
                <option value="">Unassigned</option>
                {members.map((member) => (
                  <option key={member.user_id} value={member.user_id}>
                    {member.username}
                  </option>
                ))}
              </select>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Blue</span>
              <select
                className="select px-2 py-2 text-sm"
                value={yellowUserId ?? ''}
                onChange={(event) => handleAssignPlayers(redUserId, event.target.value || null)}
                disabled={!canControl}
              >
                <option value="">Unassigned</option>
                {members.map((member) => (
                  <option key={member.user_id} value={member.user_id}>
                    {member.username}
                  </option>
                ))}
              </select>
            </label>

            <button
              type="button"
              className="btn-secondary w-full px-3 py-2 text-sm"
              onClick={handleRandomSeats}
              disabled={!canControl}
            >
              Random Seats
            </button>
          </>
        ) : (
          <>
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide muted">AI Opponent</p>
              <p className="text-xs muted">Play against a server AI. Choose a difficulty and your side.</p>
            </div>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Difficulty</span>
              <select
                className="select px-2 py-2 text-sm"
                value={aiDifficulty}
                onChange={(event) => {
                  const nextDifficulty = event.target.value as AiDifficulty;
                  setAiDifficulty(nextDifficulty);
                  if (sidePanelTab === 'ai') {
                    sendAiConfig(true, nextDifficulty, humanColorPreference);
                  }
                }}
                disabled={!canControl}
              >
                {AI_DIFFICULTY_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">You Play As</span>
              <select
                className="select px-2 py-2 text-sm"
                value={humanColorPreference}
                onChange={(event) => {
                  const nextPreference = event.target.value as ConnectFourHumanColorPreference;
                  setHumanColorPreference(nextPreference);
                  if (sidePanelTab === 'ai') {
                    sendAiConfig(true, aiDifficulty, nextPreference);
                  }
                }}
                disabled={!canControl}
              >
                <option value="red">Red</option>
                <option value="blue">Blue</option>
                <option value="random">Random</option>
              </select>
            </label>
          </>
        )}

        <div className="space-y-1 text-xs muted">
          <p>Red: {redSeatName}</p>
          <p>Blue: {blueSeatName}</p>
          <p>Turn owner: {turnOwnerName}</p>
          {aiEnabled && aiColor && <p>AI side: {aiColor === 'red' ? 'Red' : 'Blue'}</p>}
        </div>

        <button
          type="button"
          className="btn-secondary w-full px-3 py-2 text-sm"
          onClick={() => sendWs({ type: 'connect_four_reset' })}
          disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
        >
          {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'Reset Board'}
        </button>
      </aside>

      {isGameOver && (
        <div className="absolute inset-0 z-20 flex items-center justify-center rounded-xl bg-black/55 p-4 backdrop-blur-[2px]">
          <div className="panel w-full max-w-md space-y-3 rounded-2xl border border-[var(--border)] p-5">
            <h3 className="text-xl font-semibold">
              {status === 'draw' ? 'Draw' : `${winnerColor === 'red' ? 'Red' : 'Blue'} won`}
            </h3>
            <p className="text-sm muted">
              {status === 'draw' ? 'Board is full with no winner.' : 'Connect Four complete.'}
            </p>
            <div className="flex justify-end">
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                onClick={() => sendWs({ type: 'connect_four_reset' })}
                disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
              >
                {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'New Game'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

type BattleshipPanelProps = {
  battleship: WsBattleshipState;
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

type BattleshipHumanColorPreference = 'blue' | 'red' | 'random';

function BattleshipGamePanel({ battleship, members, currentUserId, canControl, sendWs }: BattleshipPanelProps) {
  const [sidePanelTab, setSidePanelTab] = useState<SidePanelTab>('local');
  const [aiDifficulty, setAiDifficulty] = useState<AiDifficulty>('medium');
  const [humanColorPreference, setHumanColorPreference] =
    useState<BattleshipHumanColorPreference>('blue');
  const [selectedShipId, setSelectedShipId] = useState<number>(1);
  const [shipOrientation, setShipOrientation] = useState<BattleshipOrientation>('horizontal');
  const blueUserId = battleship.blue_user_id ?? null;
  const redUserId = battleship.red_user_id ?? null;
  const aiEnabled = battleship.ai_enabled === true;
  const aiDifficultyValue: AiDifficulty =
    battleship.ai_difficulty === 'easy' || battleship.ai_difficulty === 'hard'
      ? battleship.ai_difficulty
      : 'medium';
  const aiColor =
    battleship.ai_color === 'blue' || battleship.ai_color === 'red' ? battleship.ai_color : null;
  const humanColorActive: 'blue' | 'red' | null = aiColor
    ? aiColor === 'blue'
      ? 'red'
      : 'blue'
    : null;
  const myColor: 'blue' | 'red' | null =
    blueUserId === currentUserId ? 'blue' : redUserId === currentUserId ? 'red' : null;
  const turnColor = battleship.turn_color === 'red' ? 'red' : 'blue';
  const isSetup = battleship.phase !== 'active' && battleship.phase !== 'finished';
  const isActive = battleship.phase === 'active' && battleship.status === 'active';
  const isFinished = battleship.phase === 'finished' || battleship.status === 'finished';
  const aiStatusLabel = aiEnabled
    ? `AI (${aiDifficultyValue[0].toUpperCase()}${aiDifficultyValue.slice(1)})`
    : null;

  const blueRows = useMemo(
    () => normalizeBattleshipRows(battleship.blue_grid_rows),
    [battleship.blue_grid_rows],
  );
  const redRows = useMemo(
    () => normalizeBattleshipRows(battleship.red_grid_rows),
    [battleship.red_grid_rows],
  );

  const ownRows = myColor === 'red' ? redRows : blueRows;
  const opponentRows = myColor === 'red' ? blueRows : redRows;
  const placedShipIds = useMemo(
    () =>
      (myColor === 'red'
        ? battleship.red_ship_ids_placed ?? []
        : battleship.blue_ship_ids_placed ?? []
      ).map((value) => Number(value)).filter((value) => Number.isInteger(value)),
    [battleship.blue_ship_ids_placed, battleship.red_ship_ids_placed, myColor],
  );

  const ownReady = myColor === 'red' ? battleship.red_ready === true : battleship.blue_ready === true;
  const canFire = canControl && !!myColor && isActive && turnColor === myColor;
  const canPlaceShips = canControl && !!myColor && isSetup;

  const hasBluePlayer = !!blueUserId;
  const hasRedPlayer = !!redUserId;
  const hasAnyAssignedPlayer = hasBluePlayer || hasRedPlayer;
  const requiresDualResetConfirm = hasBluePlayer && hasRedPlayer && blueUserId !== redUserId;
  const canRequestBoardReset = !hasAnyAssignedPlayer || !!myColor;
  const myResetRequested =
    myColor === 'blue'
      ? battleship.reset_requested_blue
      : myColor === 'red'
        ? battleship.reset_requested_red
        : false;

  const winnerColor =
    battleship.winner_color === 'red' ? 'red' : battleship.winner_color === 'blue' ? 'blue' : null;
  const blueSeatName = aiColor === 'blue' ? aiStatusLabel ?? 'AI' : nameForUser(blueUserId, members);
  const redSeatName = aiColor === 'red' ? aiStatusLabel ?? 'AI' : nameForUser(redUserId, members);
  const turnOwnerName = turnColor === 'blue' ? blueSeatName : redSeatName;

  useEffect(() => {
    if (aiEnabled) {
      setSidePanelTab('ai');
    }
  }, [aiEnabled]);

  useEffect(() => {
    setAiDifficulty(aiDifficultyValue);
    if (humanColorActive) {
      setHumanColorPreference(humanColorActive);
    }
  }, [aiDifficultyValue, humanColorActive]);

  useEffect(() => {
    if (placedShipIds.includes(selectedShipId)) {
      const nextUnplacedShip = BATTLESHIP_FLEET.find((ship) => !placedShipIds.includes(ship.id));
      if (nextUnplacedShip) {
        setSelectedShipId(nextUnplacedShip.id);
      }
    }
  }, [placedShipIds, selectedShipId]);

  function handleAssignPlayers(nextBlue: string | null, nextRed: string | null) {
    if (!canControl) return;
    sendWs({
      type: 'battleship_set_players',
      blue_user_id: nextBlue || null,
      red_user_id: nextRed || null,
    });
  }

  function handleRandomSeats() {
    if (!canControl) return;
    const assignment = randomSeatAssignment(members);
    if (!assignment) return;
    handleAssignPlayers(assignment.firstUserId, assignment.secondUserId);
  }

  function sendAiConfig(
    enabled: boolean,
    nextDifficulty: AiDifficulty = aiDifficulty,
    nextHumanColorPreference: BattleshipHumanColorPreference = humanColorPreference,
  ) {
    if (!canControl) return;
    const resolvedHumanColor: 'blue' | 'red' =
      nextHumanColorPreference === 'random'
        ? Math.random() < 0.5
          ? 'blue'
          : 'red'
        : nextHumanColorPreference;
    sendWs({
      type: 'battleship_configure_ai',
      enabled,
      difficulty: nextDifficulty,
      human_color: enabled ? resolvedHumanColor : undefined,
    });
  }

  function handleSelectSidePanelTab(nextTab: SidePanelTab) {
    setSidePanelTab(nextTab);
    if (nextTab === 'ai' && !aiEnabled) {
      sendAiConfig(true);
      return;
    }
    if (nextTab === 'local' && aiEnabled) {
      sendAiConfig(false);
    }
  }

  function handleFire(x: number, y: number) {
    if (!canFire) return;
    sendWs({ type: 'battleship_fire', x, y });
  }

  function handlePlaceShip(x: number, y: number) {
    if (!canPlaceShips) return;
    sendWs({
      type: 'battleship_place_ship',
      ship_id: selectedShipId,
      x,
      y,
      orientation: shipOrientation,
    });
  }

  function tokenClass(token: string, hideShips: boolean, shipColor: 'blue' | 'red'): string {
    if (token === 'x') return 'bg-red-500/80 border-red-300';
    if (token === 'o') return 'bg-white/25 border-white/30';
    if (token === 's' && !hideShips) {
      return shipColor === 'blue'
        ? 'bg-sky-500/70 border-sky-300'
        : 'bg-red-500/70 border-red-300';
    }
    return 'bg-black/30 border-white/10';
  }

  return (
    <div className="relative grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
      <div className="space-y-3">
        <div className="panel-soft rounded-xl p-3 sm:p-4">
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <p className="mb-2 text-xs uppercase tracking-wide muted">
                {myColor ? 'Your Fleet' : 'Blue Fleet'}
              </p>
              <div className="grid grid-cols-10 gap-1 rounded-lg border border-white/10 bg-black/20 p-2">
                {(myColor ? ownRows : blueRows).flatMap((row, y) =>
                  row.map((token, x) => (
                    <button
                      key={`own-${x}-${y}`}
                      type="button"
                      className={`aspect-square rounded border ${tokenClass(
                        token,
                        false,
                        myColor ?? 'blue',
                      )} ${
                        canPlaceShips
                          ? 'ring-1 ring-transparent transition hover:ring-[var(--orange-soft)]'
                          : ''
                      }`}
                      onClick={() => handlePlaceShip(x, y)}
                      disabled={!canPlaceShips}
                      title={`${x + 1},${y + 1}`}
                    />
                  )),
                )}
              </div>
            </div>

            <div>
              <p className="mb-2 text-xs uppercase tracking-wide muted">
                {myColor ? 'Target Grid' : 'Red Fleet'}
              </p>
              <div className="grid grid-cols-10 gap-1 rounded-lg border border-white/10 bg-black/20 p-2">
                {(myColor ? opponentRows : redRows).flatMap((row, y) =>
                  row.map((token, x) => {
                    const displayToken = myColor && token === 's' ? '.' : token;
                    const isUnknownTarget = displayToken === '.';
                    const clickable = !!myColor && canFire && isUnknownTarget;
                    return (
                      <button
                        key={`target-${x}-${y}`}
                        type="button"
                        className={`aspect-square rounded border ${tokenClass(
                          displayToken,
                          true,
                          myColor === 'red' ? 'blue' : 'red',
                        )} ${
                          clickable
                            ? 'ring-1 ring-transparent transition hover:ring-[var(--orange-soft)]'
                            : ''
                        }`}
                        onClick={() => handleFire(x, y)}
                        disabled={!clickable}
                        title={`${x + 1},${y + 1}`}
                      />
                    );
                  }),
                )}
              </div>
            </div>
          </div>
        </div>

        <div className="panel-soft rounded-xl px-3 py-2 text-xs muted">
          {battleship.last_shot ? (
            <p>
              Last shot: {battleship.last_shot.by_color.toUpperCase()} fired at{' '}
              {battleship.last_shot.x + 1},{battleship.last_shot.y + 1} ({battleship.last_shot.result}).
            </p>
          ) : (
            <p>No shots fired yet.</p>
          )}
        </div>
      </div>

      <aside className="panel-soft space-y-3 rounded-xl p-3 sm:p-4">
        <div className="space-y-2">
          <div className="flex gap-2">
            <button
              type="button"
              className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                sidePanelTab === 'local' ? 'btn-primary' : 'btn-secondary'
              }`}
              onClick={() => handleSelectSidePanelTab('local')}
            >
              Local
            </button>
            <button
              type="button"
              className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium ${
                sidePanelTab === 'ai' ? 'btn-primary' : 'btn-secondary'
              }`}
              onClick={() => handleSelectSidePanelTab('ai')}
            >
              AI
            </button>
          </div>
        </div>

        {sidePanelTab === 'local' ? (
          <>
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide muted">Players</p>
              <p className="text-xs muted">Assign Blue/Red seats, place ships, then mark ready.</p>
            </div>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Blue</span>
              <select
                className="select px-2 py-2 text-sm"
                value={blueUserId ?? ''}
                onChange={(event) => handleAssignPlayers(event.target.value || null, redUserId)}
                disabled={!canControl}
              >
                <option value="">Unassigned</option>
                {members.map((member) => (
                  <option key={member.user_id} value={member.user_id}>
                    {member.username}
                  </option>
                ))}
              </select>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Red</span>
              <select
                className="select px-2 py-2 text-sm"
                value={redUserId ?? ''}
                onChange={(event) => handleAssignPlayers(blueUserId, event.target.value || null)}
                disabled={!canControl}
              >
                <option value="">Unassigned</option>
                {members.map((member) => (
                  <option key={member.user_id} value={member.user_id}>
                    {member.username}
                  </option>
                ))}
              </select>
            </label>

            <button
              type="button"
              className="btn-secondary w-full px-3 py-2 text-sm"
              onClick={handleRandomSeats}
              disabled={!canControl}
            >
              Random Seats
            </button>
          </>
        ) : (
          <>
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide muted">AI Opponent</p>
              <p className="text-xs muted">Play against a server AI. Choose a difficulty and your side.</p>
            </div>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Difficulty</span>
              <select
                className="select px-2 py-2 text-sm"
                value={aiDifficulty}
                onChange={(event) => {
                  const nextDifficulty = event.target.value as AiDifficulty;
                  setAiDifficulty(nextDifficulty);
                  if (sidePanelTab === 'ai') {
                    sendAiConfig(true, nextDifficulty, humanColorPreference);
                  }
                }}
                disabled={!canControl}
              >
                {AI_DIFFICULTY_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">You Play As</span>
              <select
                className="select px-2 py-2 text-sm"
                value={humanColorPreference}
                onChange={(event) => {
                  const nextPreference = event.target.value as BattleshipHumanColorPreference;
                  setHumanColorPreference(nextPreference);
                  if (sidePanelTab === 'ai') {
                    sendAiConfig(true, aiDifficulty, nextPreference);
                  }
                }}
                disabled={!canControl}
              >
                <option value="blue">Blue</option>
                <option value="red">Red</option>
                <option value="random">Random</option>
              </select>
            </label>
          </>
        )}

        <button
          type="button"
          className="btn-secondary w-full px-3 py-2 text-sm"
          onClick={() => sendWs({ type: 'battleship_auto_place' })}
          disabled={!canControl || !myColor || !isSetup}
        >
          Auto Place Ships
        </button>

        <div className="space-y-2 rounded-lg border border-white/10 bg-black/20 p-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs uppercase tracking-wide muted">Manual Fleet Placement</p>
            <button
              type="button"
              className="btn-secondary px-3 py-1.5 text-xs"
              onClick={() =>
                setShipOrientation((current) =>
                  current === 'horizontal' ? 'vertical' : 'horizontal',
                )
              }
              disabled={!canPlaceShips}
            >
              {shipOrientation === 'horizontal' ? 'Horizontal' : 'Vertical'}
            </button>
          </div>
          <div className="grid gap-2">
            {BATTLESHIP_FLEET.map((ship) => {
              const placed = placedShipIds.includes(ship.id);
              const selected = selectedShipId === ship.id;
              return (
                <button
                  key={ship.id}
                  type="button"
                  className={`flex items-center justify-between rounded-lg px-3 py-2 text-sm ${
                    selected ? 'btn-primary' : 'btn-secondary'
                  } ${placed ? 'opacity-100' : ''}`}
                  onClick={() => setSelectedShipId(ship.id)}
                  disabled={!canPlaceShips}
                >
                  <span>{ship.name}</span>
                  <span className="text-xs">{placed ? 'Placed' : `${ship.size} cells`}</span>
                </button>
              );
            })}
          </div>
          <p className="text-xs muted">
            Select a ship, choose its orientation, then click your fleet grid to place or move it.
          </p>
        </div>

        <button
          type="button"
          className="btn-primary w-full px-3 py-2 text-sm disabled:opacity-45"
          onClick={() => sendWs({ type: 'battleship_set_ready', ready: !ownReady })}
          disabled={!canControl || !myColor || !isSetup}
        >
          {ownReady ? 'Mark Not Ready' : 'Mark Ready'}
        </button>

        <div className="space-y-1 text-xs muted">
          <p>Blue: {blueSeatName} {battleship.blue_ready ? '(Ready)' : ''}</p>
          <p>Red: {redSeatName} {battleship.red_ready ? '(Ready)' : ''}</p>
          <p>Turn owner: {turnOwnerName}</p>
          {aiEnabled && aiColor && <p>AI side: {aiColor === 'blue' ? 'Blue' : 'Red'}</p>}
          <p>
            Blue {isSetup ? 'ship cells placed' : 'cells left'}: {battleship.remaining_ship_cells_blue}
          </p>
          <p>
            Red {isSetup ? 'ship cells placed' : 'cells left'}: {battleship.remaining_ship_cells_red}
          </p>
        </div>

        <button
          type="button"
          className="btn-secondary w-full px-3 py-2 text-sm"
          onClick={() => sendWs({ type: 'battleship_reset' })}
          disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
        >
          {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'Reset Board'}
        </button>
      </aside>

      {isFinished && (
        <div className="absolute inset-0 z-20 flex items-center justify-center rounded-xl bg-black/55 p-4 backdrop-blur-[2px]">
          <div className="panel w-full max-w-md space-y-3 rounded-2xl border border-[var(--border)] p-5">
            <h3 className="text-xl font-semibold">
              {winnerColor ? `${winnerColor.toUpperCase()} won` : 'Game finished'}
            </h3>
            <p className="text-sm muted">Start a new Battleship round when both players are ready.</p>
            <div className="flex justify-end">
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                onClick={() => sendWs({ type: 'battleship_reset' })}
                disabled={!canRequestBoardReset || (requiresDualResetConfirm && myResetRequested)}
              >
                {requiresDualResetConfirm && myResetRequested ? 'Waiting for Opponent…' : 'New Game'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default function PlayTogetherChess({
  playState,
  members,
  currentUserId,
  canControl,
  sendWs,
}: Props) {
  if (!playState) {
    return (
      <section className="panel space-y-3 p-5 sm:p-6">
        <h2 className="text-xl font-semibold">Play Together</h2>
        <p className="text-sm muted">Connecting to shared game state…</p>
      </section>
    );
  }

  const activeGame: PlayGameKey =
    playState.active_game === 'connect_four'
      ? 'connect_four'
      : playState.active_game === 'battleship'
        ? 'battleship'
        : 'chess';

  const chess = playState.chess;
  const connectFour = playState.connect_four;
  const battleship = playState.battleship;

  const badges: string[] = [];
  if (activeGame === 'chess') {
    badges.push(`Status: ${displayChessStatus(chess.status)}`);
    badges.push(`Turn: ${chess.turn === 'black' ? 'Black' : 'White'}`);
    if (chess.winner_color) {
      badges.push(`Winner: ${chess.winner_color === 'black' ? 'Black' : 'White'}`);
    }
  } else if (activeGame === 'connect_four') {
    badges.push(`Status: ${displayConnectFourStatus(connectFour.status)}`);
    badges.push(`Turn: ${connectFour.turn === 'yellow' ? 'Blue' : 'Red'}`);
    if (connectFour.winner_color) {
      badges.push(`Winner: ${connectFour.winner_color === 'yellow' ? 'Blue' : 'Red'}`);
    }
  } else {
    badges.push(`Phase: ${displayBattleshipPhase(battleship.phase)}`);
    badges.push(`Turn: ${battleship.turn_color === 'red' ? 'Red' : 'Blue'}`);
    if (battleship.winner_color) {
      badges.push(`Winner: ${battleship.winner_color === 'red' ? 'Red' : 'Blue'}`);
    }
  }

  return (
    <section className="panel relative mt-[55px] p-5 pt-[60px] sm:p-6 sm:pt-[64px]">
      <div className="absolute left-4 right-4 top-[-17px] z-10 -translate-y-[62%] sm:left-6 sm:right-6">
        <div className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 gap-y-2">
          <div className="flex flex-wrap items-end gap-2 self-start">
            <button
              type="button"
              className="rounded-t-lg border border-b-0 border-[var(--border)] bg-[var(--surface)] px-5 py-2.5 text-sm font-medium"
              onClick={() => sendWs({ type: 'play_set_game', game: 'chess' })}
              disabled={activeGame === 'chess'}
            >
              Chess
            </button>
            <button
              type="button"
              className="rounded-t-lg border border-b-0 border-[var(--border)] bg-[var(--surface)] px-5 py-2.5 text-sm font-medium"
              onClick={() => sendWs({ type: 'play_set_game', game: 'connect_four' })}
              disabled={activeGame === 'connect_four'}
            >
              Connect Four
            </button>
            <button
              type="button"
              className="rounded-t-lg border border-b-0 border-[var(--border)] bg-[var(--surface)] px-5 py-2.5 text-sm font-medium"
              onClick={() => sendWs({ type: 'play_set_game', game: 'battleship' })}
              disabled={activeGame === 'battleship'}
            >
              Battleship
            </button>
          </div>

          <div className="min-w-0 flex flex-wrap items-center justify-end gap-2 self-start">
            {badges.map((badge) => (
              <span key={badge} className="chip">
                {badge}
              </span>
            ))}
          </div>
        </div>
      </div>

      <div className="relative space-y-4">
        {activeGame === 'chess' ? (
          <ChessGamePanel
            chess={chess}
            members={members}
            currentUserId={currentUserId}
            canControl={canControl}
            sendWs={sendWs}
          />
        ) : activeGame === 'connect_four' ? (
          <ConnectFourGamePanel
            connectFour={connectFour}
            members={members}
            currentUserId={currentUserId}
            canControl={canControl}
            sendWs={sendWs}
          />
        ) : (
          <BattleshipGamePanel
            battleship={battleship}
            members={members}
            currentUserId={currentUserId}
            canControl={canControl}
            sendWs={sendWs}
          />
        )}
      </div>
    </section>
  );
}
