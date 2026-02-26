'use client';

import { useMemo, useState } from 'react';
import { WsPlayStateMessage, WsPresenceMember } from '@/lib/watchPartyApi';

type Props = {
  playState: WsPlayStateMessage | null;
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

const FILES = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'] as const;
const RANKS = ['8', '7', '6', '5', '4', '3', '2', '1'] as const;

const PIECE_SYMBOLS: Record<string, string> = {
  P: '♙',
  N: '♘',
  B: '♗',
  R: '♖',
  Q: '♕',
  K: '♔',
  p: '♟',
  n: '♞',
  b: '♝',
  r: '♜',
  q: '♛',
  k: '♚',
};

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

function displayStatus(status: string): string {
  if (status === 'checkmate') return 'Checkmate';
  if (status === 'stalemate') return 'Stalemate';
  return 'Active';
}

function nameForUser(userId: string | null | undefined, members: WsPresenceMember[]): string {
  if (!userId) return 'Unassigned';
  return members.find((member) => member.user_id === userId)?.username ?? userId;
}

export default function PlayTogetherChess({
  playState,
  members,
  currentUserId,
  canControl,
  sendWs,
}: Props) {
  const [selectedSquare, setSelectedSquare] = useState<string | null>(null);

  const chess = playState?.chess;
  const board = useMemo(() => parseFenBoard(chess?.fen ?? ''), [chess?.fen]);
  const whiteUserId = chess?.white_user_id ?? null;
  const blackUserId = chess?.black_user_id ?? null;
  const turn = chess?.turn === 'black' ? 'black' : 'white';
  const turnUserId = turn === 'white' ? whiteUserId : blackUserId;
  const isMyTurnSeatAssigned = turnUserId ? turnUserId === currentUserId : canControl;
  const canMove = canControl && isMyTurnSeatAssigned && chess?.status === 'active';

  const selectedPiece = selectedSquare ? board.get(selectedSquare) : undefined;

  function handleAssignPlayers(nextWhite: string | null, nextBlack: string | null) {
    if (!canControl) return;
    sendWs({
      type: 'chess_set_players',
      white_user_id: nextWhite || null,
      black_user_id: nextBlack || null,
    });
  }

  function handleSquareClick(square: string) {
    if (!chess || !canMove) return;
    const clickedPiece = board.get(square);

    if (!selectedSquare) {
      if (!clickedPiece) return;
      setSelectedSquare(square);
      return;
    }

    if (selectedSquare === square) {
      setSelectedSquare(null);
      return;
    }

    if (clickedPiece) {
      setSelectedSquare(square);
      return;
    }

    const isPawnPromotion =
      selectedPiece &&
      (selectedPiece === 'P' || selectedPiece === 'p') &&
      ((selectedPiece === 'P' && square.endsWith('8')) || (selectedPiece === 'p' && square.endsWith('1')));

    sendWs({
      type: 'chess_move',
      from: selectedSquare,
      to: square,
      promotion: isPawnPromotion ? 'q' : null,
    });
    setSelectedSquare(null);
  }

  if (!chess) {
    return (
      <section className="panel space-y-3 p-5 sm:p-6">
        <h2 className="text-xl font-semibold">Play Together</h2>
        <p className="text-sm muted">Connecting to shared game state…</p>
      </section>
    );
  }

  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-end gap-2">
          <button
            type="button"
            className="rounded-t-lg border border-b-0 border-[var(--border)] bg-[var(--surface)] px-5 py-2.5 text-sm font-medium"
            onClick={() => sendWs({ type: 'play_set_game', game: 'chess' })}
            disabled={playState?.active_game === 'chess'}
          >
            Chess
          </button>
          <button
            type="button"
            className="rounded-t-lg border border-b-0 border-[var(--border)] px-5 py-2.5 text-sm font-medium opacity-50"
            disabled
          >
            More Soon
          </button>
        </div>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
          <span className="chip">Status: {displayStatus(chess.status)}</span>
          <span className="chip">Turn: {turn === 'white' ? 'White' : 'Black'}</span>
          {chess.winner_color && (
            <span className="chip">Winner: {chess.winner_color === 'white' ? 'White' : 'Black'}</span>
          )}
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
        <div className="panel-soft rounded-xl p-3 sm:p-4">
          <div className="grid grid-cols-8 overflow-hidden rounded-lg border border-white/10">
            {RANKS.flatMap((rank, rankIndex) =>
              FILES.map((file, fileIndex) => {
                const square = `${file}${rank}`;
                const piece = board.get(square);
                const isLight = (rankIndex + fileIndex) % 2 === 0;
                const isSelected = selectedSquare === square;
                const isLastMove =
                  chess.last_move_from === square || chess.last_move_to === square;
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
                    <span className={piece && piece === piece.toLowerCase() ? 'text-white/90' : 'text-white'}>
                      {piece ? PIECE_SYMBOLS[piece] : ''}
                    </span>
                    <span className="pointer-events-none absolute bottom-0.5 right-1 text-[9px] text-white/35">
                      {square}
                    </span>
                  </button>
                );
              }),
            )}
          </div>
        </div>

        <aside className="panel-soft space-y-3 rounded-xl p-3 sm:p-4">
          <div className="space-y-1">
            <p className="text-xs uppercase tracking-wide muted">Players</p>
            <p className="text-xs muted">
              Assign white and black seats, or leave unassigned for open turns.
            </p>
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

          <div className="space-y-1 text-xs muted">
            <p>White: {nameForUser(whiteUserId, members)}</p>
            <p>Black: {nameForUser(blackUserId, members)}</p>
            {turnUserId && <p>Current turn owner: {nameForUser(turnUserId, members)}</p>}
          </div>

          <button
            type="button"
            className="btn-secondary w-full px-3 py-2 text-sm"
            onClick={() => sendWs({ type: 'chess_reset' })}
            disabled={!canControl}
          >
            Reset Board
          </button>
        </aside>
      </div>
    </section>
  );
}
