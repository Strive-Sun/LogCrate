import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from 'react';
import type {
  AiAnalysisResult,
  AiAttachmentSummary,
  AiHistoryMessage,
  AiHistorySummary,
} from '../api';

export type AiDisplayResult = Omit<AiAnalysisResult, 'timing'> &
  Partial<Pick<AiAnalysisResult, 'timing'>>;

export interface AiPendingLogSnippet {
  id: number;
  sourceName: string;
  content: string;
  charCount: number;
  preview: string;
}

interface AiRequestTarget {
  providerId: string;
  model: string;
}

export interface AiWorkspaceValue {
  aiResult: AiDisplayResult | null;
  setAiResult: Dispatch<SetStateAction<AiDisplayResult | null>>;
  aiRequestTarget: AiRequestTarget | null;
  setAiRequestTarget: Dispatch<SetStateAction<AiRequestTarget | null>>;
  aiBusy: boolean;
  setAiBusy: Dispatch<SetStateAction<boolean>>;
  aiError: string | null;
  setAiError: Dispatch<SetStateAction<string | null>>;
  aiIncomplete: boolean;
  setAiIncomplete: Dispatch<SetStateAction<boolean>>;
  aiHistory: AiHistorySummary[];
  setAiHistory: Dispatch<SetStateAction<AiHistorySummary[]>>;
  aiHistoryOpen: boolean;
  setAiHistoryOpen: Dispatch<SetStateAction<boolean>>;
  aiConversation: AiHistoryMessage[];
  setAiConversation: Dispatch<SetStateAction<AiHistoryMessage[]>>;
  aiConversationText: string;
  setAiConversationText: Dispatch<SetStateAction<string>>;
  aiQuestion: string;
  setAiQuestion: Dispatch<SetStateAction<string>>;
  aiAttachments: AiAttachmentSummary[];
  setAiAttachments: Dispatch<SetStateAction<AiAttachmentSummary[]>>;
  aiLogSnippets: AiPendingLogSnippet[];
  setAiLogSnippets: Dispatch<SetStateAction<AiPendingLogSnippet[]>>;
  aiHistoryId: string | null;
  setAiHistoryId: Dispatch<SetStateAction<string | null>>;
  aiPanelOpen: boolean;
  setAiPanelOpen: Dispatch<SetStateAction<boolean>>;
  aiSendingRef: MutableRefObject<boolean>;
  aiRequestGeneration: MutableRefObject<number>;
  aiDeltaFrame: MutableRefObject<number | null>;
  aiDeltaBuffer: MutableRefObject<string>;
  aiStreamingMessageIndex: MutableRefObject<number>;
  aiSuccessfulConversation: MutableRefObject<AiHistoryMessage[]>;
  aiLogSnippetId: MutableRefObject<number>;
}

const AiWorkspaceContext = createContext<AiWorkspaceValue | null>(null);

export function AiWorkspaceProvider({ children }: { children: ReactNode }) {
  const [aiResult, setAiResult] = useState<AiDisplayResult | null>(null);
  const [aiRequestTarget, setAiRequestTarget] = useState<AiRequestTarget | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const [aiIncomplete, setAiIncomplete] = useState(false);
  const [aiHistory, setAiHistory] = useState<AiHistorySummary[]>([]);
  const [aiHistoryOpen, setAiHistoryOpen] = useState(false);
  const [aiConversation, setAiConversation] = useState<AiHistoryMessage[]>([]);
  const [aiConversationText, setAiConversationText] = useState('');
  const [aiQuestion, setAiQuestion] = useState('');
  const [aiAttachments, setAiAttachments] = useState<AiAttachmentSummary[]>([]);
  const [aiLogSnippets, setAiLogSnippets] = useState<AiPendingLogSnippet[]>([]);
  const [aiHistoryId, setAiHistoryId] = useState<string | null>(null);
  const [aiPanelOpen, setAiPanelOpen] = useState(false);
  const aiSendingRef = useRef(false);
  const aiRequestGeneration = useRef(0);
  const aiDeltaFrame = useRef<number | null>(null);
  const aiDeltaBuffer = useRef('');
  const aiStreamingMessageIndex = useRef(-1);
  const aiSuccessfulConversation = useRef<AiHistoryMessage[]>([]);
  const aiLogSnippetId = useRef(0);

  useEffect(
    () => () => {
      aiRequestGeneration.current += 1;
      if (aiDeltaFrame.current !== null) window.cancelAnimationFrame(aiDeltaFrame.current);
      aiDeltaFrame.current = null;
      aiDeltaBuffer.current = '';
    },
    [],
  );

  return (
    <AiWorkspaceContext.Provider
      value={{
        aiResult,
        setAiResult,
        aiRequestTarget,
        setAiRequestTarget,
        aiBusy,
        setAiBusy,
        aiError,
        setAiError,
        aiIncomplete,
        setAiIncomplete,
        aiHistory,
        setAiHistory,
        aiHistoryOpen,
        setAiHistoryOpen,
        aiConversation,
        setAiConversation,
        aiConversationText,
        setAiConversationText,
        aiQuestion,
        setAiQuestion,
        aiAttachments,
        setAiAttachments,
        aiLogSnippets,
        setAiLogSnippets,
        aiHistoryId,
        setAiHistoryId,
        aiPanelOpen,
        setAiPanelOpen,
        aiSendingRef,
        aiRequestGeneration,
        aiDeltaFrame,
        aiDeltaBuffer,
        aiStreamingMessageIndex,
        aiSuccessfulConversation,
        aiLogSnippetId,
      }}
    >
      {children}
    </AiWorkspaceContext.Provider>
  );
}

export function useAiWorkspace(): AiWorkspaceValue {
  const workspace = useContext(AiWorkspaceContext);
  if (!workspace) throw new Error('useAiWorkspace must be used within AiWorkspaceProvider');
  return workspace;
}
